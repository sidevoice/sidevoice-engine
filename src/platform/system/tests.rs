use super::OS;

#[test]
fn it_names_the_system_the_way_the_host_reports_it() {
    if cfg!(web) {
        assert_eq!(OS, "web");
    } else {
        assert_eq!(OS, std::env::consts::OS);
    }
}
