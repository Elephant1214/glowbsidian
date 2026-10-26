use super::*;
use crate::proto::datagram::Datagram;

const MTU: u16 = 1492;

fn flush(send: &mut SessionSend, now: Instant) -> Vec<(U24, Frame)> {
    let mut actions = Vec::new();
    send.flush(MTU, now, &mut actions);
    let mut out = Vec::new();
    for a in actions {
        let Action::Send(bytes) = a else {
            panic!("expected send")
        };
        let Datagram::Data { seq, frames } = Datagram::decode(bytes).unwrap() else {
            panic!("expected data")
        };
        out.extend(frames.into_iter().map(|f| (seq, f)));
    }
    out
}

#[test]
fn large_body_is_split_into_reliable_parts() {
    let mut s = SessionSend::default();
    let body = Bytes::from(vec![7u8; 4000]);
    s.push(body.clone(), Reliability::Unreliable, MTU);
    let frames = flush(&mut s, Instant::now());
    assert!(frames.len() > 1);

    let count = frames.len() as u32;
    let mut indexes = Vec::new();
    let mut joined = Vec::new();
    for (i, (_, f)) in frames.iter().enumerate() {
        assert_eq!(f.reliability, Reliability::Reliable); // upgraded for split
        let info = f.split.unwrap();
        assert_eq!((info.count, info.index), (count, i as u32));
        indexes.push(f.reliable_index.unwrap());
        joined.extend_from_slice(&f.body);
    }
    indexes.dedup();
    assert_eq!(indexes.len(), frames.len()); // every part has its own index
    assert_eq!(joined, body);
}

#[test]
fn datagrams_fit_the_mtu() {
    let mut s = SessionSend::default();
    for _ in 0..50 {
        s.push(Bytes::from(vec![0u8; 300]), Reliability::Reliable, MTU);
    }
    let mut actions = Vec::new();
    s.flush(MTU, Instant::now(), &mut actions);
    for a in actions {
        let Action::Send(b) = a else { panic!() };
        assert!(b.len() + IP_UDP_HEADER_LEN <= MTU as usize);
    }
}

#[test]
fn ack_clears_in_flight() {
    let mut s = SessionSend::default();
    let now = Instant::now();
    s.push(Bytes::from_static(b"x"), Reliability::Reliable, MTU);
    let seq = flush(&mut s, now)[0].0;
    s.on_ack(&[(seq, seq)], now);
    assert!(s.is_idle());
}

#[test]
fn nak_resends_a_frame_only_once() {
    let mut s = SessionSend::default();
    let now = Instant::now();
    s.push(Bytes::from_static(b"x"), Reliability::Reliable, MTU);
    let seq = flush(&mut s, now)[0].0;

    s.on_nak(&[(seq, seq)]);
    let resent = flush(&mut s, now);
    assert_eq!(resent.len(), 1);
    let seq2 = resent[0].0;

    s.on_nak(&[(seq2, seq2)]);
    assert!(flush(&mut s, now).is_empty());
    assert!(!s.is_idle()); // still waiting on the RTO
}

#[test]
fn rto_resends_after_the_nak_resend_is_used() {
    let mut s = SessionSend::default();
    let now = Instant::now();
    s.push(Bytes::from_static(b"x"), Reliability::Reliable, MTU);
    let seq = flush(&mut s, now)[0].0;
    s.on_nak(&[(seq, seq)]);
    flush(&mut s, now);

    let later = now + Duration::from_secs(3); // past the 2s RTO ceiling
    s.resend_due(later);
    assert_eq!(flush(&mut s, later).len(), 1);
}

#[test]
fn unacked_session_becomes_stuck() {
    let mut s = SessionSend::default();
    let now = Instant::now();
    let timeout = Duration::from_secs(10);
    s.push(Bytes::from_static(b"x"), Reliability::Reliable, MTU);
    flush(&mut s, now);
    assert!(!s.is_stuck(now + Duration::from_secs(9), timeout));
    assert!(s.is_stuck(now + Duration::from_secs(11), timeout));
}
