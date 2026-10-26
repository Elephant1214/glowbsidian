use super::*;
use std::net::{Ipv4Addr, SocketAddrV4, SocketAddrV6};

const CLIENT_GUID: u64 = 42;

fn v4(last: u8) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, last), 50000))
}

fn ctx(counts: Counts, existing: Option<ExistingSession>) -> OfflineContext {
    OfflineContext { counts, existing }
}

fn handler(cfg: Config) -> OfflineHandler {
    let mut h = OfflineHandler::new(&cfg);
    h.set_pong(Bytes::from_static(b"MCPE;test;"));
    h
}

fn request1(h: &mut OfflineHandler, from: SocketAddr, now: Instant) -> Option<u32> {
    let msg = OfflineMessage::OpenConnectionRequest1 {
        protocol: 8,
        mtu: 1400,
    };
    match h.handle(from, msg, &ctx(Counts::default(), None), now) {
        OfflineOutcome::Reply(OfflineMessage::OpenConnectionReply1 { cookie, .. }) => cookie,
        _ => panic!("expected REPLY_1"),
    }
}

fn request2(cookie: Option<u32>) -> OfflineMessage {
    OfflineMessage::OpenConnectionRequest2 {
        cookie,
        server_addr: v4(1),
        mtu: 1400,
        client_guid: CLIENT_GUID,
    }
}

#[test]
fn ip_bucket_groups_ipv6_by_64() {
    let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
    let b: IpAddr = "2001:db8:1:2:bbbb::9".parse().unwrap();
    let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
    assert_eq!(ip_bucket(a), ip_bucket(b));
    assert_ne!(ip_bucket(a), ip_bucket(c));
}

#[test]
fn ip_bucket_keeps_ipv4_and_unmaps_v4_in_v6() {
    let v4: IpAddr = "10.0.0.1".parse().unwrap();
    let mapped: IpAddr = "::ffff:10.0.0.1".parse().unwrap();
    assert_eq!(ip_bucket(v4), v4);
    assert_eq!(ip_bucket(mapped), v4);
}

#[test]
fn full_handshake_creates_a_session() {
    let mut h = handler(Config::default());
    let now = Instant::now();
    let cookie = request1(&mut h, v4(2), now);
    let out = h.handle(v4(2), request2(cookie), &ctx(Counts::default(), None), now);
    assert!(matches!(
        out,
        OfflineOutcome::CreateSession {
            guid: CLIENT_GUID,
            ..
        }
    ));
}

#[test]
fn request2_without_request1_is_ignored() {
    let mut h = handler(Config {
        cookies: false,
        ..Config::default()
    });
    let out = h.handle(
        v4(2),
        request2(None),
        &ctx(Counts::default(), None),
        Instant::now(),
    );
    assert!(matches!(out, OfflineOutcome::Ignore));
}

#[test]
fn wrong_cookie_is_ignored() {
    let mut h = handler(Config::default());
    let now = Instant::now();
    let cookie = request1(&mut h, v4(2), now).unwrap();
    let out = h.handle(
        v4(2),
        request2(Some(cookie.wrapping_add(1))),
        &ctx(Counts::default(), None),
        now,
    );
    assert!(matches!(out, OfflineOutcome::Ignore));
}

#[test]
fn cookie_is_bound_to_the_address() {
    let mut h = handler(Config::default());
    let now = Instant::now();
    let cookie = request1(&mut h, v4(2), now);
    request1(&mut h, v4(3), now);
    let out = h.handle(v4(3), request2(cookie), &ctx(Counts::default(), None), now);
    assert!(matches!(out, OfflineOutcome::Ignore));
}

#[test]
fn stale_request1_is_ignored() {
    let mut h = handler(Config::default());
    let now = Instant::now();
    let cookie = request1(&mut h, v4(2), now);
    let later = now + HANDSHAKE_TTL + Duration::from_secs(1);
    let out = h.handle(
        v4(2),
        request2(cookie),
        &ctx(Counts::default(), None),
        later,
    );
    assert!(matches!(out, OfflineOutcome::Ignore));
}

#[test]
fn lost_reply2_is_resent_for_the_same_guid() {
    let mut h = handler(Config::default());
    let now = Instant::now();
    let cookie = request1(&mut h, v4(2), now);
    let existing = Some(ExistingSession {
        guid: CLIENT_GUID,
        connected: false,
    });
    let out = h.handle(
        v4(2),
        request2(cookie),
        &ctx(Counts::default(), existing),
        now,
    );
    assert!(matches!(
        out,
        OfflineOutcome::Reply(OfflineMessage::OpenConnectionReply2 { .. })
    ));
}

#[test]
fn new_handshake_cannot_replace_a_connected_session() {
    let mut h = handler(Config::default());
    let now = Instant::now();
    let cookie = request1(&mut h, v4(2), now);
    let existing = Some(ExistingSession {
        guid: CLIENT_GUID + 1,
        connected: true,
    });
    let out = h.handle(
        v4(2),
        request2(cookie),
        &ctx(Counts::default(), existing),
        now,
    );
    assert!(matches!(out, OfflineOutcome::Ignore));
}

#[test]
fn unsupported_protocol_is_refused() {
    let mut h = handler(Config::default());
    let msg = OfflineMessage::OpenConnectionRequest1 {
        protocol: 6,
        mtu: 1400,
    };
    let out = h.handle(v4(2), msg, &ctx(Counts::default(), None), Instant::now());
    assert!(matches!(
        out,
        OfflineOutcome::Refuse {
            reason: RefusalReason::BadProtocol(6),
            reply: OfflineMessage::IncompatibleProtocol { protocol: 8, .. },
        }
    ));
}

#[test]
fn protocol_7_over_ipv6_is_refused() {
    let mut h = handler(Config {
        protocols: vec![7, 8],
        ..Config::default()
    });
    let from = SocketAddr::V6(SocketAddrV6::new(
        "2001:db8::1".parse().unwrap(),
        50000,
        0,
        0,
    ));
    let msg = OfflineMessage::OpenConnectionRequest1 {
        protocol: 7,
        mtu: 1400,
    };
    let out = h.handle(from, msg, &ctx(Counts::default(), None), Instant::now());
    assert!(matches!(out, OfflineOutcome::Refuse { .. }));
}

#[test]
fn refusal_reasons() {
    let h = handler(Config {
        max_sessions: 2,
        max_per_ip: 1,
        trusted_ips: vec!["10.0.0.9".parse().unwrap()],
        ..Config::default()
    });
    let counts = |connected, from_ip| Counts {
        pending: 0,
        connected,
        from_ip,
    };

    assert_eq!(h.refusal_reason(v4(2), &ctx(counts(0, 0), None)), None);
    assert_eq!(
        h.refusal_reason(v4(2), &ctx(counts(0, 1), None)),
        Some(RefusalReason::TooManyFromIp)
    );
    assert_eq!(h.refusal_reason(v4(9), &ctx(counts(0, 1), None)), None); // trusted
    assert_eq!(
        h.refusal_reason(v4(9), &ctx(counts(2, 1), None)),
        Some(RefusalReason::ServerFull) // trusted doesn't bypass max_sessions
    );
}

#[test]
fn open_only_ping_is_ignored_when_full() {
    let mut h = handler(Config {
        max_sessions: 1,
        ..Config::default()
    });
    let ping = |open_only| OfflineMessage::UnconnectedPing {
        time: 0,
        client_guid: CLIENT_GUID,
        open_only,
    };
    let full = Counts {
        pending: 0,
        connected: 1,
        from_ip: 0,
    };
    let out = h.handle(v4(2), ping(true), &ctx(full, None), Instant::now());
    assert!(matches!(out, OfflineOutcome::Ignore));
    let full = Counts {
        pending: 0,
        connected: 1,
        from_ip: 0,
    };
    let out = h.handle(v4(2), ping(false), &ctx(full, None), Instant::now());
    assert!(matches!(out, OfflineOutcome::Reply(_)));
}
