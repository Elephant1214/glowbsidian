use super::*;

fn frame(reliability: Reliability, split: Option<SplitInfo>) -> Frame {
    Frame {
        reliability,
        reliable_index: reliability.is_reliable().then(|| U24::new(10)),
        sequence_index: reliability.is_sequenced().then(|| U24::new(20)),
        order: reliability.has_order().then(|| (U24::new(30), 0)),
        split,
        body: Bytes::from_static(b"hello"),
    }
}

fn round_trip(f: &Frame) -> Frame {
    let mut buf = BytesMut::new();
    f.encode(&mut buf);
    assert_eq!(buf.len(), f.encoded_len());
    let mut bytes = buf.freeze();
    let out = Frame::decode(&mut bytes).unwrap();
    assert!(bytes.is_empty());
    out
}

#[test]
fn every_reliability_round_trips() {
    for r in [
        Reliability::Unreliable,
        Reliability::UnreliableSequenced,
        Reliability::Reliable,
        Reliability::ReliableOrdered,
    ] {
        let f = frame(r, None);
        let out = round_trip(&f);
        assert_eq!(out.reliability, r);
        assert_eq!(out.reliable_index, f.reliable_index);
        assert_eq!(out.sequence_index, f.sequence_index);
        assert_eq!(out.order, f.order);
        assert_eq!(out.body, f.body);
    }
}

#[test]
fn split_round_trips() {
    let info = SplitInfo {
        count: 3,
        id: 9,
        index: 2,
    };
    let out = round_trip(&frame(Reliability::ReliableOrdered, Some(info)));
    let s = out.split.unwrap();
    assert_eq!((s.count, s.id, s.index), (3, 9, 2));
}

fn encoded(f: &Frame) -> Bytes {
    let mut buf = BytesMut::new();
    f.encode(&mut buf);
    buf.freeze()
}

#[test]
fn split_index_past_count_is_rejected() {
    let info = SplitInfo {
        count: 2,
        id: 0,
        index: 2,
    };
    let mut bytes = encoded(&frame(Reliability::Reliable, Some(info)));
    assert!(Frame::decode(&mut bytes).is_err());
}

#[test]
fn nonzero_channel_is_rejected() {
    let mut f = frame(Reliability::ReliableOrdered, None);
    f.order = Some((U24::new(0), 1));
    assert!(Frame::decode(&mut encoded(&f)).is_err());
}

#[test]
fn empty_body_is_rejected() {
    let mut f = frame(Reliability::Unreliable, None);
    f.body = Bytes::new();
    assert!(Frame::decode(&mut encoded(&f)).is_err());
}

#[test]
fn truncated_body_is_rejected() {
    let mut bytes = encoded(&frame(Reliability::Unreliable, None));
    let mut short = bytes.split_to(bytes.len() - 1);
    assert!(Frame::decode(&mut short).is_err());
}
