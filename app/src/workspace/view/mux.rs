use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Clipped, Container, CornerRadius, CrossAxisAlignment, Element, Flex, Hoverable,
    MouseStateHandle, Padding, ParentElement, Radius, Text,
};
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, EntityId, SingletonEntity, ViewContext};

use crate::appearance::Appearance;
use crate::pane_group::{NewTerminalOptions, PanesLayout};
use crate::workspace::mux::core::{Manager, Session, attach_command, valid_host};
use crate::workspace::mux::{self, Discovery};
use crate::workspace::{Workspace, WorkspaceAction};

#[derive(Default)]
struct HostState {
    expanded: bool,
    loading: bool,
    discovery: Option<Discovery>,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct MuxPanelState {
    hosts: BTreeMap<String, HostState>,
    importing: bool,
    import_error: Option<String>,
    connections: HashMap<(String, Manager, String), EntityId>,
    mouse_states: RefCell<HashMap<String, MouseStateHandle>>,
}

impl Workspace {
    pub(super) fn import_mux_hosts(&mut self, ctx: &mut ViewContext<Self>) {
        if self.mux_panel.importing {
            return;
        }
        self.mux_panel.importing = true;
        ctx.spawn(
            async {
                tokio::task::spawn_blocking(|| {
                    let home = dirs::home_dir()
                        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
                    mux::config::import_hosts(&home)
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|result| result)
            },
            |workspace, result, ctx| {
                let state = &mut workspace.mux_panel;
                state.importing = false;
                match result {
                    Ok(hosts) => {
                        state.hosts.retain(|host, _| hosts.contains(host));
                        for host in hosts {
                            state.hosts.entry(host).or_default();
                        }
                        state.import_error = None;
                        state.mouse_states.borrow_mut().clear();
                    }
                    Err(error) => state.import_error = Some(format!("{error:#}")),
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    pub(super) fn toggle_mux_host(&mut self, host: &str, ctx: &mut ViewContext<Self>) {
        let Some(state) = self.mux_panel.hosts.get_mut(host) else {
            return;
        };
        state.expanded = !state.expanded;
        if state.expanded && state.discovery.is_none() {
            self.refresh_mux_host(host, ctx);
        }
        ctx.notify();
    }

    pub(super) fn refresh_mux_host(&mut self, host: &str, ctx: &mut ViewContext<Self>) {
        let Some(state) = self.mux_panel.hosts.get_mut(host) else {
            return;
        };
        if state.loading {
            return;
        }
        state.loading = true;
        state.expanded = true;
        state.error = None;
        let host = host.to_owned();
        let destination = host.clone();
        ctx.spawn(
            async move { mux::discover(&destination).await },
            move |workspace, result, ctx| {
                if let Some(state) = workspace.mux_panel.hosts.get_mut(&host) {
                    state.loading = false;
                    match result {
                        Ok(discovery) => state.discovery = Some(discovery),
                        Err(error) => state.error = Some(format!("{error:#}")),
                    }
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    pub(super) fn open_mux_session(
        &mut self,
        host: &str,
        session: Option<&Session>,
        ctx: &mut ViewContext<Self>,
    ) {
        if !valid_host(host) || !self.mux_panel.hosts.contains_key(host) {
            return;
        }
        if let Some(session) = session {
            let key = (host.to_owned(), session.manager, session.id.clone());
            if let Some(tab_id) = self.mux_panel.connections.get(&key) {
                if let Some(index) = self
                    .tabs
                    .iter()
                    .position(|tab| tab.pane_group.id() == *tab_id)
                {
                    self.activate_tab(index, ctx);
                    return;
                }
                self.mux_panel.connections.remove(&key);
            }
        }
        let title = match session {
            Some(session) => format!("{host} / {} / {}", session.manager.label(), session.name),
            None => format!("SSH / {host}"),
        };
        self.add_tab_with_pane_layout(
            PanesLayout::SingleTerminal(Box::new(NewTerminalOptions {
                hide_homepage: true,
                ..Default::default()
            })),
            Arc::new(HashMap::new()),
            Some(title),
            ctx,
        );
        let group = self.active_tab_pane_group().clone();
        if let Some(terminal) = group.as_ref(ctx).active_session_view(ctx) {
            terminal.update(ctx, |view, ctx| {
                let shell = view.shell_family(ctx);
                let destination = shell.shell_escape(host);
                let mut command =
                    format!("command /usr/bin/ssh -tt -o RemoteCommand=none -- {destination}");
                if let Some(session) = session {
                    let remote = mux::remote_command(&attach_command(session));
                    command.push(' ');
                    command.push_str(&shell.shell_escape(&remote));
                }
                view.set_pending_command(&command, ctx);
            });
            if let Some(session) = session {
                self.mux_panel.connections.insert(
                    (host.to_owned(), session.manager, session.id.clone()),
                    group.id(),
                );
            }
        }
        ctx.notify();
    }

    pub(super) fn render_mux_hosts(&self, app: &AppContext) -> Box<dyn Element> {
        let state = &self.mux_panel;
        let appearance = Appearance::as_ref(app);
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(mux_label("SSH HOSTS", appearance));
        column.add_child(mux_button(
            state,
            "import",
            if state.importing {
                "Reading SSH config…"
            } else {
                "Import / reload ~/.ssh/config"
            },
            WorkspaceAction::ImportMuxHosts,
            appearance,
        ));
        if let Some(error) = &state.import_error {
            column.add_child(mux_label(error, appearance));
        } else if state.hosts.is_empty() && !state.importing {
            column.add_child(mux_label(
                "Add a Host alias to ~/.ssh/config, then import.",
                appearance,
            ));
        }
        for (host, host_state) in &state.hosts {
            let marker = if host_state.expanded { "▾" } else { "▸" };
            let label = if host_state.loading {
                format!("{marker} {host} · Loading…")
            } else {
                format!("{marker} {host}")
            };
            column.add_child(mux_button(
                state,
                &format!("host:{host}"),
                &label,
                WorkspaceAction::ToggleMuxHost(host.clone()),
                appearance,
            ));
            if !host_state.expanded {
                continue;
            }
            let mut children =
                Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            children.add_child(mux_button(
                state,
                &format!("ssh:{host}"),
                "Open SSH (authenticate)",
                WorkspaceAction::OpenMuxSsh(host.clone()),
                appearance,
            ));
            children.add_child(mux_button(
                state,
                &format!("refresh:{host}"),
                "Refresh sessions",
                WorkspaceAction::RefreshMuxHost(host.clone()),
                appearance,
            ));
            if let Some(error) = &host_state.error {
                children.add_child(mux_label(error, appearance));
            }
            if let Some(discovery) = &host_state.discovery {
                for session in &discovery.sessions {
                    let label = format!("{} · {}", session.manager.label(), session.name);
                    children.add_child(mux_button(
                        state,
                        &format!("session:{host}:{}:{}", session.manager.label(), session.id),
                        &label,
                        WorkspaceAction::OpenMuxSession {
                            host: host.clone(),
                            session: session.clone(),
                        },
                        appearance,
                    ));
                }
                if discovery.sessions.is_empty() {
                    children.add_child(mux_label("No persistent sessions", appearance));
                }
                for notice in &discovery.notices {
                    children.add_child(mux_label(notice, appearance));
                }
            }
            column.add_child(
                Container::new(children.finish())
                    .with_padding(Padding::uniform(0.).with_left(12.))
                    .finish(),
            );
        }
        column.add_child(mux_label("OPEN TABS", appearance));
        column.finish()
    }
}

fn mux_label(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        Text::new(text.to_owned(), appearance.ui_font_family(), 11.)
            .with_color(
                appearance
                    .theme()
                    .sub_text_color(appearance.theme().background())
                    .into(),
            )
            .finish(),
    )
    .with_padding(Padding::uniform(6.))
    .finish()
}

fn mux_button(
    state: &MuxPanelState,
    id: &str,
    label: &str,
    action: WorkspaceAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let mouse = state
        .mouse_states
        .borrow_mut()
        .entry(id.to_owned())
        .or_default()
        .clone();
    let theme = appearance.theme();
    let label = label.to_owned();
    Hoverable::new(mouse, move |hover| {
        let content = Text::new_inline(label, appearance.ui_font_family(), 12.)
            .with_clip(ClipConfig::ellipsis())
            .with_color(theme.main_text_color(theme.background()).into())
            .finish();
        let mut row = Container::new(Clipped::new(content).finish())
            .with_padding(Padding::uniform(6.))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
        if hover.is_hovered() {
            row = row.with_background(internal_colors::fg_overlay_2(theme));
        }
        row.finish()
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .with_cursor(Cursor::PointingHand)
    .finish()
}
