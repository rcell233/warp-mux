use std::collections::BTreeMap;

pub type HostId = u64;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Local,
    Ssh(String),
}
impl Target {
    pub fn label(&self) -> &str {
        match self {
            Self::Local => "Local",
            Self::Ssh(alias) => alias,
        }
    }
    pub fn ssh_alias(&self) -> Option<&str> {
        match self {
            Self::Local => None,
            Self::Ssh(alias) => Some(alias),
        }
    }
}

pub struct HostTab<T> {
    pub id: HostId,
    pub target: Target,
    pub anchor: T,
    pub active: T,
    pub members: Vec<T>,
    pub sessions: BTreeMap<String, T>,
}

pub struct Navigation<T> {
    pub hosts: Vec<HostTab<T>>,
    pub selected: HostId,
    next_id: HostId,
}
impl<T: Copy + Eq> Navigation<T> {
    pub fn new(anchor: T) -> Self {
        Self {
            hosts: vec![HostTab {
                id: 0,
                target: Target::Local,
                anchor,
                active: anchor,
                members: vec![anchor],
                sessions: BTreeMap::new(),
            }],
            selected: 0,
            next_id: 1,
        }
    }
    pub fn selected(&self) -> &HostTab<T> {
        self.hosts
            .iter()
            .find(|host| host.id == self.selected)
            .expect("Selected host exists")
    }
    pub fn host(&self, id: HostId) -> Option<&HostTab<T>> {
        self.hosts.iter().find(|host| host.id == id)
    }
    pub fn add_host(&mut self, target: Target, anchor: T) -> HostId {
        let id = self.next_id;
        self.next_id += 1;
        self.hosts.push(HostTab {
            id,
            target,
            anchor,
            active: anchor,
            members: vec![anchor],
            sessions: BTreeMap::new(),
        });
        self.selected = id;
        id
    }
    pub fn register(&mut self, host_id: HostId, native: T, session: Option<String>) {
        if let Some(host) = self.hosts.iter_mut().find(|host| host.id == host_id) {
            if !host.members.contains(&native) {
                host.members.push(native);
            }
            if let Some(session) = session {
                host.sessions.insert(session, native);
            }
            host.active = native;
            self.selected = host_id;
        }
    }
    pub fn observe(&mut self, native: T) {
        if let Some(host) = self
            .hosts
            .iter_mut()
            .find(|host| host.members.contains(&native))
        {
            host.active = native;
            self.selected = host.id;
        }
    }
    pub fn forget(&mut self, native: T) {
        for host in &mut self.hosts {
            host.members.retain(|member| *member != native);
            host.sessions.retain(|_, member| *member != native);
            if host.anchor == native
                && let Some(first) = host.members.first()
            {
                host.anchor = *first;
            }
            if host.active == native {
                host.active = host.anchor;
            }
        }
        self.hosts.retain(|host| !host.members.is_empty());
        if self.host(self.selected).is_none()
            && let Some(host) = self.hosts.first()
        {
            self.selected = host.id;
        }
    }
}
#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
