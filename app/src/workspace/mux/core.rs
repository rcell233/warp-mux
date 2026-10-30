#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Manager {
    Zmx,
    Tmux,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zmx => "zmx",
            Self::Tmux => "tmux",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Session {
    pub manager: Manager,
    pub id: String,
    pub name: String,
}

pub fn parse_sessions(manager: Manager, text: &str) -> Vec<Session> {
    text.lines()
        .filter_map(|line| {
            let (id, name) = match manager {
                Manager::Zmx => {
                    let name = line
                        .trim_start()
                        .split('\t')
                        .find_map(|field| field.strip_prefix("name="))?;
                    (name, name)
                }
                Manager::Tmux => {
                    let (id, name) = line.split_once('|')?;
                    if !id.strip_prefix('$').is_some_and(|value| {
                        !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit())
                    }) {
                        return None;
                    }
                    (id, name)
                }
            };
            if name.is_empty() || name.chars().any(char::is_control) {
                return None;
            }
            Some(Session {
                manager,
                id: id.to_owned(),
                name: name.to_owned(),
            })
        })
        .collect()
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

pub fn attach_command(session: &Session) -> String {
    let target = quote(&session.id);
    match session.manager {
        Manager::Zmx => format!("exec zmx attach {target}"),
        Manager::Tmux => format!("exec tmux -u attach-session -t {target}"),
    }
}

pub fn no_sessions_error(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("no server running")
        || text.contains("no sessions")
        || (text.contains("error connecting")
            && (text.contains("no such file") || text.contains("connection refused")))
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
