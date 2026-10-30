use super::*;
#[test]
fn preserves_unicode_spaces_and_equals_in_zmx_names() {
    assert_eq!(
        parse_sessions("name=工作 = api\tpid=12\tclients=0\r\n"),
        vec![Session {
            name: "工作 = api".into()
        }]
    );
}
#[test]
fn ignores_control_characters_and_deduplicates_names() {
    assert_eq!(
        parse_sessions("name=\tpid=1\nname=bad\u{1b}[31m\nname=logs\tpid=1\nname=logs\tpid=1\n"),
        vec![Session {
            name: "logs".into()
        }]
    );
}
#[test]
fn quotes_shell_metacharacters_as_literal_data() {
    assert_eq!(
        quote("a'b;$(printf injected)"),
        "'a'\"'\"'b;$(printf injected)'"
    );
}
#[test]
fn rejects_host_patterns_and_option_injection() {
    assert!(!valid_host("-oProxyCommand=evil"));
    assert!(!valid_host("*.example.org"));
    assert!(!valid_host("!excluded"));
    assert!(!valid_host("host\nother"));
    assert!(valid_host("prod-server"));
}
