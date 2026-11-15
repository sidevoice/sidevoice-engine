use super::{Current, System};

#[test]
fn it_names_the_system_the_way_the_host_reports_it() {
    assert!(!Current::OS.is_empty());
    if cfg!(web) {
        assert_eq!(Current::OS, "web");
    } else {
        assert_eq!(Current::OS, std::env::consts::OS);
    }
}
