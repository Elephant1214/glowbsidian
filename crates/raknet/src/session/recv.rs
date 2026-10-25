use crate::data_types::U24;
use crate::proto::IP_UDP_HEADER_LEN;
use crate::proto::datagram::{encode_ranges, to_ranges};
use crate::proto::frame::{Frame, SplitInfo};
use crate::proto::reliable::{Reliability, ReliableWindow};
use crate::session::{Action, SessionConfig};
use bytes::{Bytes, BytesMut};
use std::collections::{HashMap, HashSet};

const MAX_NAK_GAP: u32 = 64;
const MAX_ORDER_HELD: usize = 1024;
const MAX_ORDER_HELD_BYTES: usize = 4 * 1024 * 1024;
const ACK_HEADER_LEN: usize = 3;
const MAX_RANGE_LEN: usize = 7;

#[derive(Default)]
pub(super) struct SessionRecv {
    /// Sequence numbers received since the last tick
    acks: Vec<U24>,
    /// Gaps detected
    naks: HashSet<U24>,
    /// Next datagram sequence expected
    expected_seq: U24,

    /// Dedupe by reliable message index
    reliable_seen: ReliableWindow,
    /// Next ordering index on channel 0
    order_expected: U24,
    /// Arrived early, waiting for the gap to fill
    order_held: HashMap<U24, Bytes>,
    /// Total body bytes in `order_held`
    order_held_bytes: usize,
    /// Unreliable/reliable sequences: drop anything older
    sequence_highest: Option<U24>,
    /// Split id -> parts being reassembled
    splits: HashMap<u16, PacketSplit>,
}

impl SessionRecv {
    pub(super) fn note_seq(&mut self, seq: U24) {
        self.acks.push(seq);
        if seq == self.expected_seq {
            self.expected_seq += 1;
        } else if seq.is_after(self.expected_seq) {
            if seq - self.expected_seq <= MAX_NAK_GAP {
                let mut missing = self.expected_seq;
                while missing != seq {
                    self.naks.insert(missing);
                    missing += 1;
                }
            }
            self.expected_seq = seq + 1;
        }
    }

    /// Runs one frame through dedupe -> split assembly -> ordering. Complete messages to go `out`.
    pub(super) fn accept(
        &mut self,
        frame: Frame,
        cfg: SessionConfig,
        out: &mut Vec<Bytes>,
    ) -> Result<(), &'static str> {
        if let Some(index) = frame.reliable_index {
            if !self.reliable_seen.insert(index) {
                return Ok(()); // This is a duplicate
            }
        }
        let frame = match frame.split {
            Some(info) => match self.add_split(info, frame, cfg)? {
                Some(whole) => whole,
                None => return Ok(()), // Still waiting for parts
            },
            None => frame,
        };

        match frame.reliability {
            Reliability::Unreliable | Reliability::Reliable => out.push(frame.body),
            Reliability::UnreliableSequenced => {
                let (Some((order, _)), Some(seq)) = (frame.order, frame.sequence_index) else {
                    return Ok(());
                };
                // Sequence frames belong to the current ordering index.
                // Anything else is stale or early and unreliable.
                if order != self.order_expected {
                    return Ok(());
                }
                if self.sequence_highest.is_some_and(|h| !seq.is_after(h)) {
                    return Ok(());
                }
                self.sequence_highest = Some(seq);
                out.push(frame.body);
            }
            Reliability::ReliableOrdered => {
                let Some((order, _)) = frame.order else {
                    return Ok(());
                };
                if order == self.order_expected {
                    out.push(frame.body);
                    self.advance_order(out);
                } else if order.is_after(self.order_expected) {
                    // A real sender can't get further ahead than the frames we're able to hold
                    if order - self.order_expected > MAX_ORDER_HELD as u32 {
                        return Err("ordering index too far ahead");
                    }
                    if self.order_held.contains_key(&order) {
                        return Ok(());
                    }
                    if self.order_held.len() >= MAX_ORDER_HELD
                        || self.order_held_bytes + frame.body.len() > MAX_ORDER_HELD_BYTES
                    {
                        return Err("too many out-of-order frames");
                    }
                    self.order_held_bytes += frame.body.len();
                    self.order_held.insert(order, frame.body);
                }
            }
        }

        Ok(())
    }

    fn advance_order(&mut self, out: &mut Vec<Bytes>) {
        self.order_expected += 1;
        self.sequence_highest = None; // The sender resets its sequence index after each ordered message
        while let Some(body) = self.order_held.remove(&self.order_expected) {
            self.order_held_bytes -= body.len();
            out.push(body);
            self.order_expected += 1;
        }
    }

    fn add_split(
        &mut self,
        info: SplitInfo,
        frame: Frame,
        cfg: SessionConfig,
    ) -> Result<Option<Frame>, &'static str> {
        if info.count == 0 || info.count > cfg.max_split_parts {
            return Err("split count out of range");
        }
        if !self.splits.contains_key(&info.id) && self.splits.len() >= cfg.max_concurrent_splits {
            return Err("too many concurrent splits");
        }

        let split = self.splits.entry(info.id).or_insert_with(|| PacketSplit {
            count: info.count,
            parts: vec![None; info.count as usize],
            received: 0,
        });
        if split.count != info.count {
            return Err("split count changed");
        }

        let slot = &mut split.parts[info.index as usize];
        if slot.is_none() {
            *slot = Some(frame.body.clone());
            split.received += 1;
        }
        if split.received < split.count {
            return Ok(None);
        }

        let split = self.splits.remove(&info.id).expect("just inserted");
        let mut body = BytesMut::new();
        for part in split.parts.into_iter().flatten() {
            body.extend_from_slice(&part);
        }
        Ok(Some(Frame {
            split: None,
            body: body.freeze(),
            ..frame
        }))
    }

    pub(super) fn flush_acks(&mut self, mtu: u16, out: &mut Vec<Action>) {
        let per_datagram = (mtu as usize - IP_UDP_HEADER_LEN - ACK_HEADER_LEN) / MAX_RANGE_LEN;
        for s in &self.acks {
            self.naks.remove(s); // if a datagram arrived after all
        }
        for (nak, seqs) in [
            (false, std::mem::take(&mut self.acks)),
            (true, std::mem::take(&mut self.naks).into_iter().collect()),
        ] {
            if seqs.is_empty() {
                continue;
            }

            for chunk in to_ranges(seqs).chunks(per_datagram) {
                let mut buf = BytesMut::new();
                encode_ranges(nak, chunk, &mut buf);
                out.push(Action::Send(buf.freeze()));
            }
        }
    }
}

pub(super) struct PacketSplit {
    count: u32,
    parts: Vec<Option<Bytes>>,
    received: u32,
}
