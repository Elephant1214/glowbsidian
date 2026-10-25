use crate::data_types::U24;
use crate::proto::IP_UDP_HEADER_LEN;
use crate::proto::datagram::{DATAGRAM_HEADER_LEN, put_data_header};
use crate::proto::frame::{Frame, SplitInfo};
use crate::proto::reliable::Reliability;
use crate::session::Action;
use bytes::{Bytes, BytesMut};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// flags + length + reliable + sequence + order + split
const MAX_FRAME_HEADER_LEN: usize = 3 + 3 + 3 + 4 + 10;
const MAX_ACK_RANGE: u32 = 4096;
/// Most sequences a single ACK or NAK datagram can the server look at.
const MAX_ACK_WORK: usize = 4096;
/// The RTO doubles per timed-out resend, up to 2^MAX_BACKOFF times.
const MAX_BACKOFF: u32 = 4;
/// Unacked bytes allowed on the wire. `flush` stops here and leaves the rest queued.
const MAX_IN_FLIGHT_BYTES: usize = 512 * 1024;
/// Queued + in-flight bytes before the peer is treated as dead.
const MAX_BACKLOG_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct SessionSend {
    /// Datagram sequence number
    next_seq: U24,
    /// Reliable message index
    next_reliable: U24,
    /// Ordering index, channel 0
    next_order: U24,
    /// Sequencing index
    next_sequence: U24,
    next_split_id: u16,

    /// Frames waiting to be packed into datagrams
    queue: VecDeque<PendingFrame>,
    /// Seq -> what was in it, for resend
    in_flight: HashMap<U24, InFlight>,
    queued_bytes: usize,
    in_flight_bytes: usize,
    stalled_since: Option<Instant>,

    /// Smoothed RTT + variance -> resent timeout
    rtt: Rtt,
    /// The total backoff for this session. This stops a peer that never ACKs from getting full
    /// resends on every retransmission timeout.
    backoff: u32,
}

impl SessionSend {
    pub(super) fn push(&mut self, body: Bytes, reliability: Reliability, mtu: u16) {
        let max_body =
            mtu as usize - IP_UDP_HEADER_LEN - DATAGRAM_HEADER_LEN - MAX_FRAME_HEADER_LEN;
        let needs_split = body.len() > max_body;
        let reliability = if needs_split {
            reliability.for_split()
        } else {
            reliability
        };

        let (sequence_index, order) = match reliability {
            Reliability::UnreliableSequenced => {
                let seq = self.next_sequence;
                self.next_sequence += 1;
                (Some(seq), Some((self.next_order, 0)))
            }
            Reliability::ReliableOrdered => {
                let order = self.next_order;
                self.next_order += 1;
                self.next_sequence = U24::default();
                (None, Some((order, 0)))
            }
            _ => (None, None),
        };

        if !needs_split {
            let reliable_index = reliability.is_reliable().then(|| self.take_reliable());
            self.queue.push_back(PendingFrame {
                frame: Frame {
                    reliability,
                    reliable_index,
                    sequence_index,
                    order,
                    split: None,
                    body,
                },
                nak_resent: false,
            });
            return;
        }

        let id = self.next_split_id;
        self.next_split_id = self.next_split_id.wrapping_add(1);
        let count = body.len().div_ceil(max_body) as u32;
        for index in 0..count {
            let start = index as usize * max_body;
            let end = (start + max_body).min(body.len());
            let reliable_index = self.take_reliable(); // Every part gets its own index
            self.queue.push_back(PendingFrame {
                frame: Frame {
                    reliability,
                    reliable_index: Some(reliable_index),
                    sequence_index,
                    order, // All parts share one order
                    split: Some(SplitInfo { count, id, index }),
                    body: body.slice(start..end),
                },
                nak_resent: false,
            });
        }
    }

    fn take_reliable(&mut self) -> U24 {
        let index = self.next_reliable;
        self.next_reliable += 1;
        index
    }

    /// Packs the queue into datagrams and records the reliable frames for resend.
    pub(super) fn flush(&mut self, mtu: u16, now: Instant, out: &mut Vec<Action>) {
        let max = mtu as usize - IP_UDP_HEADER_LEN;
        while !self.queue.is_empty() && self.in_flight_bytes < MAX_IN_FLIGHT_BYTES {
            let seq = self.next_seq;
            self.next_seq += 1;
            let mut buf = BytesMut::with_capacity(max);
            put_data_header(&mut buf, seq);

            let mut reliable = Vec::new();
            while let Some(pending) = self.queue.front() {
                if buf.len() + pending.frame.encoded_len() > max && buf.len() > DATAGRAM_HEADER_LEN
                {
                    break; // full, the rest goes into the next datagram
                }
                let pending = self.queue.pop_front().expect("front exist");
                self.queued_bytes -= pending.frame.body.len();
                pending.frame.encode(&mut buf);
                if pending.frame.reliability.is_reliable() {
                    reliable.push(pending);
                }
            }

            if !reliable.is_empty() {
                self.in_flight_bytes += body_len(&reliable);
                self.stalled_since.get_or_insert(now);
                self.in_flight.insert(
                    seq,
                    InFlight {
                        frames: reliable,
                        sent_at: now,
                    },
                );
            }
            out.push(Action::Send(buf.freeze()));
        }
    }

    pub(super) fn resend_due(&mut self, now: Instant) {
        let rto = self.rtt.rto() * (1 << self.backoff);
        let due: Vec<U24> = self
            .in_flight
            .iter()
            .filter(|(_, f)| now.saturating_duration_since(f.sent_at) >= rto)
            .map(|(seq, _)| *seq)
            .collect();
        if !due.is_empty() {
            self.backoff = (self.backoff + 1).min(MAX_BACKOFF);
        }
        for seq in due {
            if let Some(in_flight) = self.in_flight.remove(&seq) {
                self.requeue(in_flight.frames);
            }
        }
    }

    pub(super) fn on_ack(&mut self, ranges: &[(U24, U24)], now: Instant) {
        let mut progress = false;

        for seq in self.in_flight_seqs(ranges) {
            if let Some(in_flight) = self.in_flight.remove(&seq) {
                self.in_flight_bytes -= body_len(&in_flight.frames);
                self.rtt
                    .sample(now.saturating_duration_since(in_flight.sent_at));
                self.backoff = 0;
                progress = true;
            }
        }

        if progress {
            self.stalled_since = (!self.in_flight.is_empty()).then_some(now);
        }
    }

    pub(super) fn on_nak(&mut self, ranges: &[(U24, U24)]) {
        for seq in self.in_flight_seqs(ranges) {
            let Some(in_flight) = self.in_flight.get_mut(&seq) else {
                continue;
            };

            // Only one NAK resend per frame so that they can't be used to lag DoS the socket
            let (mut fresh, used): (Vec<_>, Vec<_>) = std::mem::take(&mut in_flight.frames)
                .into_iter()
                .partition(|p| !p.nak_resent);
            if used.is_empty() {
                self.in_flight.remove(&seq);
            } else {
                in_flight.frames = used;
            }

            for pending in &mut fresh {
                pending.nak_resent = true;
            }

            self.requeue(fresh);
        }
    }

    /// The in-flight sequences covered by `ranges`. Each range is either walked or matched against
    /// `in_flight`, whichever is cheaper, and the total work per datagram is capped at
    /// MAX_ACK_WORK so a packet full of huge ranges can't make the server loop millions of times.
    ///
    /// Ranges longer than MAX_ACK_RANGE (including reversed ones) are skipped.
    fn in_flight_seqs(&self, ranges: &[(U24, U24)]) -> Vec<U24> {
        let mut budget = MAX_ACK_WORK;
        let mut seqs = Vec::new();
        for &(min, max) in ranges {
            let span = max - min;
            if span > MAX_ACK_RANGE {
                continue;
            }
            let cost = (span as usize + 1).min(self.in_flight.len());
            if cost > budget {
                break;
            }
            budget -= cost;

            if (span as usize) < self.in_flight.len() {
                let mut seq = min;
                loop {
                    if self.in_flight.contains_key(&seq) {
                        seqs.push(seq);
                    }
                    if seq == max {
                        break;
                    }
                    seq += 1;
                }
            } else {
                seqs.extend(self.in_flight.keys().filter(|&&s| s - min <= span));
            }
        }
        seqs
    }

    /// Puts a lost datagram's reliable frames back at the front. They go out in a new datagram
    /// with a new seq and a fresh sent_at timestamp so that RTT samples stay valid.
    fn requeue(&mut self, frames: Vec<PendingFrame>) {
        let bytes = body_len(&frames);
        self.in_flight_bytes -= bytes;
        self.queued_bytes += bytes;
        for frame in frames.into_iter().rev() {
            self.queue.push_front(frame);
        }
    }

    pub(super) fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.in_flight.is_empty()
    }

    pub(super) fn is_stuck(&self, now: Instant, timeout: Duration) -> bool {
        self.queued_bytes + self.in_flight_bytes > MAX_BACKLOG_BYTES
            || self
                .stalled_since
                .is_some_and(|t| now.saturating_duration_since(t) > timeout)
    }
}

pub(super) struct InFlight {
    frames: Vec<PendingFrame>,
    sent_at: Instant,
}

pub(super) struct PendingFrame {
    frame: Frame,
    nak_resent: bool,
}

fn body_len(frames: &[PendingFrame]) -> usize {
    frames.iter().map(|p| p.frame.body.len()).sum()
}

/// Estimate of round-trip time
#[derive(Default)]
pub struct Rtt {
    /// Smoothed RTT, `None` until the first sample
    srtt: Option<Duration>,
    rttvar: Duration,
}

impl Rtt {
    fn sample(&mut self, r: Duration) {
        match self.srtt {
            None => {
                self.srtt = Some(r);
                self.rttvar = r / 2;
            }
            Some(s) => {
                let diff = s.abs_diff(r);
                self.rttvar = self.rttvar * 3 / 4 + diff / 4;
                self.srtt = Some(s * 7 / 8 + r / 8);
            }
        }
    }

    fn rto(&self) -> Duration {
        let rto = match self.srtt {
            None => Duration::from_millis(500),
            Some(s) => s + self.rttvar * 4,
        };
        rto.clamp(Duration::from_millis(50), Duration::from_secs(2))
    }
}
