use super::*;
#[test]
fn host_switching_remembers_each_hosts_active_window() {
    let mut nav = Navigation::new(1);
    let prod = nav.add_host(Target::Ssh("prod".into()), 2);
    nav.register(prod, 3, Some("api".into()));
    nav.register(prod, 4, Some("logs".into()));
    nav.observe(1);
    assert_eq!(nav.selected, 0);
    assert_eq!(nav.host(prod).unwrap().active, 4);
    nav.observe(3);
    assert_eq!(nav.selected, prod);
    assert_eq!(nav.selected().active, 3);
}
#[test]
fn same_window_name_on_different_hosts_stays_independent() {
    let mut nav = Navigation::new(1);
    let a = nav.add_host(Target::Ssh("a".into()), 2);
    nav.register(a, 3, Some("dev".into()));
    let b = nav.add_host(Target::Ssh("b".into()), 4);
    nav.register(b, 5, Some("dev".into()));
    assert_eq!(nav.host(a).unwrap().sessions.get("dev"), Some(&3));
    assert_eq!(nav.host(b).unwrap().sessions.get("dev"), Some(&5));
}
#[test]
fn closing_window_keeps_other_windows_and_reconciles_selection() {
    let mut nav = Navigation::new(1);
    nav.register(0, 2, Some("dev".into()));
    nav.forget(1);
    assert_eq!(nav.selected().anchor, 2);
    assert_eq!(nav.selected().active, 2);
}
