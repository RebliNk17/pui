use super::*;
#[test]
fn full_pool_fails_before_helper_or_raster_ack_and_failure_keeps_only_source() {
    let ctx = egui::Context::default();
    let occupied = budget::Lease::new(&ctx);
    occupied.reserve(64 * 1024 * 1024).unwrap();
    let lease = budget::Lease::new(&ctx);
    let source = "<svg width='2' height='1'/>".to_owned();
    let mut called = false;
    let failed = Preview::load_using(source.clone(), Some(&lease), None, |_, _, _| {
        called = true;
        unreachable!()
    });
    assert!(!called);
    assert!(failed.error.is_some());
    drop(occupied);
    let occupied = budget::Lease::new(&ctx);
    occupied
        .reserve(64 * 1024 * 1024 - source.capacity())
        .unwrap();
    let mut acked = false;
    let failed = Preview::load_using(source.clone(), Some(&lease), None, |_, _, reserve| {
        reserve(8)?;
        acked = true;
        unreachable!()
    });
    assert!(!acked);
    assert_eq!(failed.retained_bytes(), source.capacity());
    assert!(failed.error.is_some());
    drop(occupied);
    drop(lease);
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
}
#[test]
fn failure_after_reservation_releases_pixels_and_arc_lease_holds_retained_source_and_pixels() {
    let ctx = egui::Context::default();
    let lease = budget::Lease::new(&ctx);
    let source = "<svg width='2' height='1'/>".to_owned();
    let failed = Preview::load_using(source.clone(), Some(&lease), None, |_, _, reserve| {
        reserve(8)?;
        Err("controlled timeout".into())
    });
    assert_eq!(failed.retained_bytes(), source.capacity());
    let other = budget::Lease::new(&ctx);
    other.reserve(64 * 1024 * 1024 - source.capacity()).unwrap();
    drop(other);
    let preview = Preview::load_using(source, Some(&lease), None, |_, _, reserve| {
        reserve(8)?;
        Ok(crate::svg_preview::Raster {
            size: [2, 1],
            rgba: vec![128, 0, 0, 128, 0, 0, 0, 0],
        })
    });
    assert!(preview.error.is_none());
    let retained = preview.retained_bytes();
    let parked = lease.clone();
    drop(lease);
    let other = budget::Lease::new(&ctx);
    other.reserve(64 * 1024 * 1024 - retained).unwrap();
    assert!(other.reserve(64 * 1024 * 1024).is_err());
    drop(parked);
    other.reserve(64 * 1024 * 1024).unwrap();
}
