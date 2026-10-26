use super::*;

#[test]
fn new_masks_to_24_bits() {
    assert_eq!(U24::new(0x1FF_FFFF).get(), 0xFF_FFFF);
}

#[test]
fn next_and_add_wrap() {
    assert_eq!(U24::new(0xFF_FFFF).next(), U24::new(0));
    assert_eq!(U24::new(0xFF_FFFE) + 3, U24::new(1));
}

#[test]
fn distance_across_wrap() {
    assert_eq!(U24::new(2) - U24::new(0xFF_FFFF), 3);
    assert_eq!(U24::new(2).ahead_of(U24::new(0xFF_FFFF)), 3);
}

#[test]
fn is_after_uses_half_range() {
    assert!(U24::new(1).is_after(U24::new(0xFF_FFFF)));
    assert!(!U24::new(0xFF_FFFF).is_after(U24::new(1)));
    assert!(!U24::new(5).is_after(U24::new(5)));
    assert!(U24::new(0x7F_FFFF).is_after(U24::new(0)));
    assert!(!U24::new(0x80_0000).is_after(U24::new(0)));
}
