use super::*;
use crate::proto::reliability::Reliability;

fn u(v: u32) -> U24 {
    U24::new(v)
}

#[test]
fn to_ranges_sorts_dedups_and_collapses() {
    let seqs = vec![u(5), u(1), u(2), u(3), u(3), u(7), u(6)];
    assert_eq!(to_ranges(seqs), vec![(u(1), u(3)), (u(5), u(7))]);
}

#[test]
fn ack_round_trip() {
    let ranges = vec![(u(1), u(1)), (u(4), u(9)), (u(0xFF_FFFF), u(0xFF_FFFF))];
    let mut buf = BytesMut::new();
    encode_ranges(false, &ranges, &mut buf);
    match Datagram::decode(buf.freeze()).unwrap() {
        Datagram::Ack(r) => assert_eq!(r, ranges),
        _ => panic!("expected ACK"),
    }
}

#[test]
fn nak_round_trip() {
    let ranges = vec![(u(3), u(4))];
    let mut buf = BytesMut::new();
    encode_ranges(true, &ranges, &mut buf);
    match Datagram::decode(buf.freeze()).unwrap() {
        Datagram::Nak(r) => assert_eq!(r, ranges),
        _ => panic!("expected NAK"),
    }
}

#[test]
fn ack_with_arrival_rate_is_skipped() {
    let mut buf = BytesMut::new();
    buf.put_u8(0xE0); // ACK + arrival rate
    buf.put_f32(1.0);
    buf.put_u16(1);
    buf.put_u8(1);
    buf.put_u24_le(u(42));
    match Datagram::decode(buf.freeze()).unwrap() {
        Datagram::Ack(r) => assert_eq!(r, vec![(u(42), u(42))]),
        _ => panic!("expected ACK"),
    }
}

#[test]
fn truncated_ack_is_an_error() {
    let mut buf = BytesMut::new();
    buf.put_u8(ACK);
    buf.put_u16(2);
    buf.put_u8(1);
    buf.put_u24_le(u(1));
    assert!(Datagram::decode(buf.freeze()).is_err());
}

#[test]
fn bad_trailing_frame_keeps_earlier_frames() {
    let mut buf = BytesMut::new();
    put_data_header(&mut buf, u(7));
    Frame {
        reliability: Reliability::Unreliable,
        reliable_index: None,
        sequence_index: None,
        order: None,
        split: None,
        body: Bytes::from_static(b"ok"),
    }
    .encode(&mut buf);
    buf.put_u8(0xE0); // reliability 7: invalid
    match Datagram::decode(buf.freeze()).unwrap() {
        Datagram::Data { seq, frames } => {
            assert_eq!(seq, u(7));
            assert_eq!(frames.len(), 1);
            assert_eq!(&frames[0].body[..], b"ok");
        }
        _ => panic!("expected data"),
    }
}
