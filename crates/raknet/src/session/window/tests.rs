use super::*;

#[test]
fn in_order_then_duplicate() {
    let mut w = ReliableWindow::default();
    assert!(w.insert(U24::new(0)));
    assert!(w.insert(U24::new(1)));
    assert!(!w.insert(U24::new(0)));
    assert!(!w.insert(U24::new(1)));
}

#[test]
fn out_of_order_fills_gap() {
    let mut w = ReliableWindow::default();
    assert!(w.insert(U24::new(2)));
    assert!(!w.insert(U24::new(2)));
    assert!(w.insert(U24::new(0)));
    assert!(w.insert(U24::new(1)));
    assert!(w.above.is_empty());
    assert_eq!(w.base, U24::new(3));
}

#[test]
fn far_ahead_is_dropped() {
    let mut w = ReliableWindow::default();
    assert!(!w.insert(U24::new(RELIABLE_WINDOW)));
    assert!(w.insert(U24::new(RELIABLE_WINDOW - 1)));
}

#[test]
fn wraps_at_24_bits() {
    let mut w = ReliableWindow {
        base: U24::new(0xFF_FFFF),
        above: HashSet::new(),
    };
    assert!(w.insert(U24::new(0)));
    assert!(w.insert(U24::new(0xFF_FFFF)));
    assert_eq!(w.base, U24::new(1));
    assert!(!w.insert(U24::new(0xFF_FFFF)));
}
