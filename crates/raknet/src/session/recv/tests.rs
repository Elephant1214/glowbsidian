use super::*;
use crate::proto::datagram::Datagram;
use std::time::Duration;

fn cfg() -> SessionConfig {
    SessionConfig {
        pending_timeout: Duration::from_secs(5),
        timeout: Duration::from_secs(10),
        max_split_parts: 4,
        max_concurrent_splits: 2,
    }
}

fn u(v: u32) -> U24 {
    U24::new(v)
}

fn ordered(order: u32, body: &'static [u8]) -> Frame {
    Frame {
        reliability: Reliability::ReliableOrdered,
        reliable_index: Some(u(order)),
        sequence_index: None,
        order: Some((u(order), 0)),
        split: None,
        body: Bytes::from_static(body),
    }
}

fn part(reliable: u32, id: u16, index: u32, count: u32, body: &'static [u8]) -> Frame {
    Frame {
        reliability: Reliability::Reliable,
        reliable_index: Some(u(reliable)),
        sequence_index: None,
        order: None,
        split: Some(SplitInfo { count, id, index }),
        body: Bytes::from_static(body),
    }
}

type Ranges = Vec<(U24, U24)>;

/// Flushes and decodes the (ack, nak) ranges that would go out.
fn flushed(recv: &mut SessionRecv) -> (Ranges, Ranges) {
    let mut actions = Vec::new();
    recv.flush_acks(1492, &mut actions);
    let (mut acks, mut naks) = (Vec::new(), Vec::new());
    for a in actions {
        let Action::Send(bytes) = a else {
            panic!("expected send")
        };
        match Datagram::decode(bytes).unwrap() {
            Datagram::Ack(r) => acks.extend(r),
            Datagram::Nak(r) => naks.extend(r),
            Datagram::Data { .. } => panic!("expected ack/nak"),
        }
    }
    (acks, naks)
}

#[test]
fn gap_is_naked() {
    let mut r = SessionRecv::default();
    r.note_seq(u(0));
    r.note_seq(u(3));
    let (acks, naks) = flushed(&mut r);
    assert_eq!(acks, vec![(u(0), u(0)), (u(3), u(3))]);
    assert_eq!(naks, vec![(u(1), u(2))]);
}

#[test]
fn late_arrival_cancels_nak() {
    let mut r = SessionRecv::default();
    r.note_seq(u(0));
    r.note_seq(u(2));
    r.note_seq(u(1));
    let (_, naks) = flushed(&mut r);
    assert!(naks.is_empty());
}

#[test]
fn huge_gap_is_not_naked() {
    let mut r = SessionRecv::default();
    r.note_seq(u(MAX_NAK_GAP + 1));
    let (_, naks) = flushed(&mut r);
    assert!(naks.is_empty());
}

#[test]
fn ordered_frames_are_held_until_gap_fills() {
    let mut r = SessionRecv::default();
    let mut out = Vec::new();
    r.accept(ordered(1, b"b"), cfg(), &mut out).unwrap();
    r.accept(ordered(2, b"c"), cfg(), &mut out).unwrap();
    assert!(out.is_empty());
    r.accept(ordered(0, b"a"), cfg(), &mut out).unwrap();
    assert_eq!(out, vec![&b"a"[..], b"b", b"c"]);
    assert_eq!(r.order_held_bytes, 0);
}

#[test]
fn duplicate_reliable_is_dropped() {
    let mut r = SessionRecv::default();
    let mut out = Vec::new();
    r.accept(ordered(0, b"a"), cfg(), &mut out).unwrap();
    r.accept(ordered(0, b"a"), cfg(), &mut out).unwrap();
    assert_eq!(out.len(), 1);
}

#[test]
fn ordering_too_far_ahead_is_an_error() {
    let mut r = SessionRecv::default();
    let mut f = ordered(0, b"x");
    f.order = Some((u(MAX_ORDER_HELD as u32 + 1), 0));
    assert!(r.accept(f, cfg(), &mut Vec::new()).is_err());
}

#[test]
fn stale_sequenced_frame_is_dropped() {
    let mut r = SessionRecv::default();
    let mut out = Vec::new();
    let seq = |s: u32, body: &'static [u8]| Frame {
        reliability: Reliability::UnreliableSequenced,
        reliable_index: None,
        sequence_index: Some(u(s)),
        order: Some((u(0), 0)),
        split: None,
        body: Bytes::from_static(body),
    };
    r.accept(seq(2, b"new"), cfg(), &mut out).unwrap();
    r.accept(seq(1, b"old"), cfg(), &mut out).unwrap();
    assert_eq!(out, vec![&b"new"[..]]);
}

#[test]
fn split_reassembles_out_of_order() {
    let mut r = SessionRecv::default();
    let mut out = Vec::new();
    r.accept(part(0, 7, 2, 3, b"c"), cfg(), &mut out).unwrap();
    r.accept(part(1, 7, 0, 3, b"a"), cfg(), &mut out).unwrap();
    assert!(out.is_empty());
    r.accept(part(2, 7, 1, 3, b"b"), cfg(), &mut out).unwrap();
    assert_eq!(out, vec![&b"abc"[..]]);
    assert!(r.splits.is_empty());
}

#[test]
fn split_limits_are_enforced() {
    let mut r = SessionRecv::default();
    let mut out = Vec::new();
    // more parts than max_split_parts
    assert!(r.accept(part(0, 1, 0, 5, b"x"), cfg(), &mut out).is_err());
    r.accept(part(1, 1, 0, 2, b"x"), cfg(), &mut out).unwrap();
    // same id, different count
    assert!(r.accept(part(2, 1, 1, 3, b"x"), cfg(), &mut out).is_err());
    r.accept(part(3, 2, 0, 2, b"x"), cfg(), &mut out).unwrap();
    // a third split while two are open
    assert!(r.accept(part(4, 3, 0, 2, b"x"), cfg(), &mut out).is_err());
}
