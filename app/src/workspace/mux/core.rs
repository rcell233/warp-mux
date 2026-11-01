#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Session {
    pub name: String,
}

pub fn parse_sessions(text: &str) -> Vec<Session> {
    let mut sessions: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let name = line
                .trim_start()
                .split('\t')
                .find_map(|field| field.strip_prefix("name="))?;
            if !valid_session_name(name) {
                return None;
            }
            Some(Session {
                name: name.to_owned(),
            })
        })
        .collect();
    sessions.sort_by(|a, b| a.name.cmp(&b.name));
    sessions.dedup();
    sessions
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('-')
        && !host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "*?![]".contains(c))
}

pub fn valid_session_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && !name.chars().any(char::is_control)
}

pub fn attach_command(session: &Session) -> String {
    format!(
        "unset ZMX_SESSION; exec zmx attach {}",
        quote(&session.name)
    )
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
