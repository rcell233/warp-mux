use super::*;

#[test]
fn zmx_preserves_spaces_equals_and_unicode_in_names() {
    let sessions = parse_sessions(
        Manager::Zmx,
        "name=工作 = api\tpid=12\tclients=0\r\nname=logs\tpid=13\n",
    );
    assert_eq!(sessions[0].name, "工作 = api");
    assert_eq!(sessions[1].id, "logs");
}

#[test]
fn tmux_uses_stable_ids_and_preserves_pipes_in_names() {
    let sessions = parse_sessions(
        Manager::Tmux,
        "$3|api|worker\n$8|日志\nnot-a-session|foo\n$|bad\n",
    );
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name, "api|worker");
    assert_eq!(
        attach_command(&sessions[0]),
        "exec tmux -u attach-session -t '$3'"
    );
}

#[test]
fn ignores_empty_and_control_character_session_names() {
    assert_eq!(
        parse_sessions(Manager::Zmx, "name=\tpid=1\nname=bad\u{1b}[31m\n"),
        vec![]
    );
}

#[test]
fn quotes_shell_metacharacters_as_literal_data() {
    assert_eq!(
        quote("a'b;$(touch /tmp/no)"),
        "'a'\"'\"'b;$(touch /tmp/no)'"
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

#[test]
fn empty_tmux_server_is_distinct_from_permission_error() {
    assert!(no_sessions_error(
        "error connecting to /tmp/tmux-1000/default (No such file or directory)"
    ));
    assert!(!no_sessions_error(
        "error connecting to /tmp/tmux-1000/default (Permission denied)"
    ));
}
