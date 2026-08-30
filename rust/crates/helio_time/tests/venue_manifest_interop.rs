use helio_scan::SessionDate;
use helio_time::VenueSchedule;

#[test]
fn python_calendar_manifest_validates_in_rust_without_translation() {
    let schedule: VenueSchedule =
        serde_json::from_str(include_str!("fixtures/xnys_2026_thanksgiving.json")).unwrap();
    schedule.validate().unwrap();
    assert!(schedule.session(SessionDate(20_783)).is_err());
    let early_close = schedule.session(SessionDate(20_784)).unwrap();
    assert_eq!(early_close.close_utc - early_close.open_utc, 12_600);
}
