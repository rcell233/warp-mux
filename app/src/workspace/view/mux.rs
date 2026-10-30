use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use ui_components::{Component as _, Options as _, button};
use warp_core::ui::Icon;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
    CornerRadius, CrossAxisAlignment, Dismiss, Element, Expanded, Flex, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    ScrollbarWidth, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{AppContext, EntityId, SingletonEntity, ViewContext, ViewHandle};

use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use crate::pane_group::{NewTerminalOptions, PanesLayout};
use crate::workspace::mux::core::{Session, attach_command, valid_session_name};
use crate::workspace::mux::hosts::SavedHosts;
use crate::workspace::mux::navigation::{HostId, Navigation, Target};
use crate::workspace::{Workspace, WorkspaceAction, mux};

mod files;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MuxDialog {
    Files,
    NewHost,
    ManageHosts,
    ImportHosts,
    NewWindow,
}

#[derive(Default)]
struct DiscoveryState {
    sessions: Vec<Session>,
    loading: bool,
    ssh_terminal_open: bool,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct MuxPanelState {
    navigation: Option<Navigation<EntityId>>,
    files: files::FileBrowser,
    window_name_editor: Option<ViewHandle<EditorView>>,
    window_name: String,
    saved: SavedHosts,
    registry_ready: bool,
    registry_error: Option<String>,
    candidates: Vec<String>,
    selected_candidates: BTreeSet<String>,
    importing: bool,
    import_error: Option<String>,
    dialog: Option<MuxDialog>,
    discovery: HashMap<HostId, DiscoveryState>,
    creating_for: Option<HostId>,
    rows: RefCell<HashMap<String, MouseStateHandle>>,
    buttons: RefCell<HashMap<String, button::Button>>,
    session_scroll: ClippedScrollStateHandle,
    dialog_scroll: ClippedScrollStateHandle,
}

fn registry_path() -> PathBuf {
    warp_core::paths::data_dir().join("mux").join("hosts.json")
}

impl Workspace {
    pub(super) fn initialize_mux(&mut self, ctx: &mut ViewContext<Self>) {
        crate::FeatureFlag::VerticalTabs.set_enabled(false);
        let anchor = self.active_tab_pane_group().id();
        let mut navigation = Navigation::new(anchor);
        for tab in &self.tabs {
            navigation.register(0, tab.pane_group.id(), None);
        }
        navigation.observe(anchor);
        self.mux_panel.navigation = Some(navigation);
        let editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(14.), appearance),
                    select_all_on_focus: true,
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text("Window name", ctx);
            editor
        });
        ctx.subscribe_to_view(&editor, |workspace, editor, event, ctx| match event {
            EditorEvent::Edited(_) => {
                workspace.mux_panel.window_name = editor.as_ref(ctx).buffer_text(ctx);
                ctx.notify();
            }
            EditorEvent::Enter => workspace.create_mux_window(ctx),
            EditorEvent::Escape => workspace.show_mux_dialog(None, ctx),
            _ => {}
        });
        self.mux_panel.window_name_editor = Some(editor);
        ctx.spawn(
            async {
                tokio::task::spawn_blocking(|| SavedHosts::load(&registry_path()))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|r| r)
            },
            |workspace, result, ctx| {
                workspace.mux_panel.registry_ready = true;
                match result {
                    Ok(saved) => workspace.mux_panel.saved = saved,
                    Err(error) => workspace.mux_panel.registry_error = Some(format!("{error:#}")),
                }
                ctx.notify();
            },
        );
        self.initialize_mux_files(ctx);
        self.refresh_mux_host(0, ctx);
    }

    pub(super) fn register_mux_native_tab(&mut self, native: EntityId) {
        let Some(nav) = self.mux_panel.navigation.as_mut() else {
            return;
        };
        let host = match self.mux_panel.creating_for {
            Some(id) => id,
            None => match nav.hosts.iter().find(|host| host.target == Target::Local) {
                Some(host) => host.id,
                None => nav.add_host(Target::Local, native),
            },
        };
        nav.register(host, native, None);
    }

    pub(super) fn observe_mux_native_tab(&mut self, native: EntityId) {
        if let Some(nav) = &mut self.mux_panel.navigation {
            nav.observe(native);
        }
    }

    pub(super) fn forget_mux_native_tab(&mut self, native: EntityId) {
        if let Some(nav) = &mut self.mux_panel.navigation {
            nav.forget(native);
        }
    }

    pub(super) fn show_mux_dialog(
        &mut self,
        dialog: Option<MuxDialog>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.mux_panel.dialog = dialog;
        if dialog.is_some() && dialog != Some(MuxDialog::NewWindow) {
            ctx.focus_self();
        }
        if dialog == Some(MuxDialog::NewWindow) {
            self.mux_panel.window_name.clear();
            if let Some(editor) = self.mux_panel.window_name_editor.clone() {
                editor.update(ctx, |view, ctx| view.set_buffer_text("", ctx));
                ctx.focus(&editor);
            }
        }
        if matches!(dialog, Some(MuxDialog::NewHost | MuxDialog::ManageHosts)) {
            match SavedHosts::load(&registry_path()) {
                Ok(saved) => {
                    self.mux_panel.saved = saved;
                    self.mux_panel.registry_error = None;
                }
                Err(error) => self.mux_panel.registry_error = Some(format!("{error:#}")),
            }
            self.mux_panel.registry_ready = true;
        }
        if dialog == Some(MuxDialog::ImportHosts) {
            self.import_mux_hosts(ctx);
        }
        if dialog.is_none() {
            self.focus_active_tab(ctx);
        }
        ctx.notify();
    }

    pub(super) fn create_mux_window(&mut self, ctx: &mut ViewContext<Self>) {
        if self.mux_panel.dialog != Some(MuxDialog::NewWindow) {
            return;
        }
        let name = self.mux_panel.window_name.trim().to_owned();
        if !valid_session_name(&name) {
            return;
        }
        let Some(host) = self.mux_panel.navigation.as_ref().map(|nav| nav.selected) else {
            return;
        };
        let state = self.mux_panel.discovery.entry(host).or_default();
        if state.sessions.iter().any(|session| session.name == name) {
            return;
        }
        let session = Session { name };
        state.sessions.push(session.clone());
        state.sessions.sort_by(|a, b| a.name.cmp(&b.name));
        self.mux_panel.dialog = None;
        self.open_mux_session(host, &session, ctx);
    }

    pub(super) fn import_mux_hosts(&mut self, ctx: &mut ViewContext<Self>) {
        if self.mux_panel.importing {
            return;
        }
        self.mux_panel.importing = true;
        self.mux_panel.import_error = None;
        self.mux_panel.selected_candidates.clear();
        ctx.spawn(
            async {
                tokio::task::spawn_blocking(|| {
                    let home = dirs::home_dir()
                        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
                    mux::config::import_hosts(&home)
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|r| r)
            },
            |workspace, result, ctx| {
                workspace.mux_panel.importing = false;
                match result {
                    Ok(hosts) => workspace.mux_panel.candidates = hosts,
                    Err(error) => workspace.mux_panel.import_error = Some(format!("{error:#}")),
                }
                ctx.notify();
            },
        );
    }

    pub(super) fn toggle_mux_candidate(&mut self, alias: &str, ctx: &mut ViewContext<Self>) {
        if !self
            .mux_panel
            .candidates
            .iter()
            .any(|candidate| candidate == alias)
            || self
                .mux_panel
                .saved
                .aliases
                .iter()
                .any(|saved| saved == alias)
        {
            return;
        }
        if !self.mux_panel.selected_candidates.remove(alias) {
            self.mux_panel.selected_candidates.insert(alias.to_owned());
        }
        ctx.notify();
    }

    pub(super) fn save_mux_hosts(&mut self, remove: Option<&str>, ctx: &mut ViewContext<Self>) {
        if !self.mux_panel.registry_ready || self.mux_panel.registry_error.is_some() {
            return;
        }
        let mut saved = match SavedHosts::load(&registry_path()) {
            Ok(saved) => saved,
            Err(error) => {
                self.mux_panel.registry_error = Some(format!("{error:#}"));
                ctx.notify();
                return;
            }
        };
        if let Some(alias) = remove {
            saved.aliases.retain(|saved| saved != alias);
        } else {
            saved.add(self.mux_panel.selected_candidates.iter().cloned());
        }
        match saved.save(&registry_path()) {
            Ok(()) => {
                self.mux_panel.saved = saved;
                self.mux_panel.dialog = Some(MuxDialog::ManageHosts);
                self.mux_panel.import_error = None;
            }
            Err(error) => {
                self.mux_panel.import_error = Some(format!("Could not save hosts: {error:#}"))
            }
        }
        ctx.notify();
    }

    pub(super) fn create_mux_host(&mut self, alias: Option<&str>, ctx: &mut ViewContext<Self>) {
        if alias.is_some_and(|alias| {
            !self
                .mux_panel
                .saved
                .aliases
                .iter()
                .any(|saved| saved == alias)
        }) {
            return;
        }
        let target = alias.map_or(Target::Local, |alias| Target::Ssh(alias.to_owned()));
        let existing = self.mux_panel.navigation.as_ref().and_then(|nav| {
            nav.hosts
                .iter()
                .find(|host| host.target == target)
                .map(|host| host.id)
        });
        self.mux_panel.dialog = None;
        if let Some(id) = existing {
            self.activate_mux_host(id, ctx);
            return;
        }
        self.add_tab_with_pane_layout(
            PanesLayout::SingleTerminal(Box::new(NewTerminalOptions {
                hide_homepage: true,
                ..Default::default()
            })),
            Arc::new(HashMap::new()),
            Some(target.label().to_owned()),
            ctx,
        );
        let anchor = self.active_tab_pane_group().id();
        let Some(nav) = self.mux_panel.navigation.as_mut() else {
            return;
        };
        // The normal native-tab hook classifies new terminals as local until their host is known.
        nav.forget(anchor);
        let id = nav.add_host(target, anchor);
        self.refresh_mux_host(id, ctx);
        ctx.notify();
    }

    pub(super) fn select_mux_host_number(&mut self, number: usize, ctx: &mut ViewContext<Self>) {
        let id = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.hosts.get(number.saturating_sub(1)))
            .map(|host| host.id);
        if let Some(id) = id {
            self.activate_mux_host(id, ctx);
        }
    }
    pub(super) fn cycle_mux_host(&mut self, forward: bool, ctx: &mut ViewContext<Self>) {
        let id = self.mux_panel.navigation.as_ref().and_then(|nav| {
            let index = nav.hosts.iter().position(|host| host.id == nav.selected)?;
            let index = if forward {
                (index + 1) % nav.hosts.len()
            } else {
                (index + nav.hosts.len() - 1) % nav.hosts.len()
            };
            Some(nav.hosts[index].id)
        });
        if let Some(id) = id {
            self.activate_mux_host(id, ctx);
        }
    }
    pub(super) fn close_selected_mux_host(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(id) = self.mux_panel.navigation.as_ref().map(|nav| nav.selected) {
            self.close_mux_host(id, ctx);
        }
    }

    pub(super) fn activate_mux_host(&mut self, host: HostId, ctx: &mut ViewContext<Self>) {
        let native = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.host(host))
            .map(|host| host.active);
        if let Some(native) = native
            && let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.pane_group.id() == native)
        {
            self.activate_tab(index, ctx);
        }
    }

    pub(super) fn close_mux_host(&mut self, host: HostId, ctx: &mut ViewContext<Self>) {
        let Some(nav) = self.mux_panel.navigation.as_ref() else {
            return;
        };
        if nav.hosts.len() == 1 {
            ctx.close_window();
            return;
        }
        let Some(group) = nav.host(host) else {
            return;
        };
        let members = group.members.clone();
        let indices: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| members.contains(&tab.pane_group.id()))
            .map(|(index, _)| index)
            .collect();
        for index in indices.into_iter().rev() {
            self.remove_tab(index, false, true, ctx);
        }
        self.mux_panel.discovery.remove(&host);
        ctx.notify();
    }

    pub(super) fn refresh_mux_host(&mut self, host: HostId, ctx: &mut ViewContext<Self>) {
        let Some(target) = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.host(host))
            .map(|host| host.target.clone())
        else {
            return;
        };
        let state = self.mux_panel.discovery.entry(host).or_default();
        if state.loading {
            return;
        }
        state.loading = true;
        state.error = None;
        ctx.spawn(
            async move { mux::discover(target.ssh_alias()).await },
            move |workspace, result, ctx| {
                if workspace
                    .mux_panel
                    .navigation
                    .as_ref()
                    .and_then(|nav| nav.host(host))
                    .is_none()
                {
                    return;
                }
                let state = workspace.mux_panel.discovery.entry(host).or_default();
                state.loading = false;
                let first = match result {
                    Ok(discovery) => {
                        state.sessions = discovery.sessions;
                        state.sessions.first().cloned()
                    }
                    Err(error) => {
                        state.error = Some(format!("{error:#}"));
                        None
                    }
                };
                let should_select =
                    workspace.mux_panel.navigation.as_ref().is_some_and(|nav| {
                        nav.selected == host && nav.selected().sessions.is_empty()
                    });
                if should_select && let Some(session) = first {
                    workspace.open_mux_session(host, &session, ctx);
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    pub(super) fn open_mux_session(
        &mut self,
        host: HostId,
        session: &Session,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(nav) = self.mux_panel.navigation.as_ref() else {
            return;
        };
        let Some(group) = nav.host(host) else {
            return;
        };
        if let Some(native) = group.sessions.get(&session.name)
            && let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.pane_group.id() == *native)
        {
            self.activate_tab(index, ctx);
            return;
        }
        let target = group.target.clone();
        self.mux_panel.creating_for = Some(host);
        self.add_tab_with_pane_layout(
            PanesLayout::SingleTerminal(Box::new(NewTerminalOptions {
                hide_homepage: true,
                ..Default::default()
            })),
            Arc::new(HashMap::new()),
            Some(session.name.clone()),
            ctx,
        );
        self.mux_panel.creating_for = None;
        let native = self.active_tab_pane_group().clone();
        if let Some(terminal) = native.as_ref(ctx).active_session_view(ctx) {
            terminal.update(ctx, |view, ctx| {
                let shell = view.shell_family(ctx);
                let remote = mux::remote_command(&attach_command(session));
                let command = match target.ssh_alias() {
                    Some(alias) => format!(
                        "command /usr/bin/ssh -tt -o RemoteCommand=none -- {} {}",
                        shell.shell_escape(alias),
                        shell.shell_escape(&remote)
                    ),
                    None => format!("command /bin/sh -c {}", shell.shell_escape(&remote)),
                };
                view.set_pending_command(&command, ctx);
            });
            if let Some(nav) = self.mux_panel.navigation.as_mut() {
                nav.register(host, native.id(), Some(session.name.clone()));
            }
        }
        ctx.notify();
    }

    pub(super) fn authenticate_mux_host(&mut self, host: HostId, ctx: &mut ViewContext<Self>) {
        let alias = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.host(host))
            .and_then(|host| host.target.ssh_alias())
            .map(str::to_owned);
        let Some(alias) = alias else {
            return;
        };
        self.mux_panel
            .discovery
            .entry(host)
            .or_default()
            .ssh_terminal_open = true;
        let Some(native) = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.host(host))
            .map(|host| host.anchor)
        else {
            return;
        };
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.pane_group.id() == native)
        {
            self.activate_tab(index, ctx);
            let pane = self.active_tab_pane_group().clone();
            if let Some(terminal) = pane.as_ref(ctx).active_session_view(ctx) {
                terminal.update(ctx, |view, ctx| {
                    let command = format!(
                        "command /usr/bin/ssh -tt -o RemoteCommand=none -- {}",
                        view.shell_family(ctx).shell_escape(&alias)
                    );
                    view.set_pending_command(&command, ctx);
                });
            }
        }
        ctx.notify();
    }

    pub(super) fn mux_files_open(&self) -> bool {
        self.mux_panel.dialog == Some(MuxDialog::Files)
    }

    pub(super) fn mux_dialog_open(&self) -> bool {
        self.mux_panel.dialog.is_some()
    }

    pub(super) fn add_mux_host_tabs(
        &self,
        bar: &mut Flex,
        hover_fixed_width: Option<f32>,
        app: &AppContext,
    ) {
        use crate::tab::{TabBarState, TabComponent};
        use crate::workspace::tab_settings::TabSettings;
        let Some(nav) = &self.mux_panel.navigation else {
            return;
        };
        let state = TabBarState {
            tab_count: nav.hosts.len(),
            active_tab_index: nav.hosts.iter().position(|host| host.id == nav.selected),
            is_any_tab_renaming: false,
            is_any_tab_dragging: false,
            hover_fixed_width,
        };
        for (index, host) in nav.hosts.iter().enumerate() {
            if let Some(tab) = self
                .tabs
                .iter()
                .find(|tab| tab.pane_group.id() == host.anchor)
            {
                bar.add_child(
                    TabComponent::new(
                        index,
                        state,
                        tab,
                        self.tab_rename_editor.clone(),
                        TabSettings::as_ref(app).close_button_position,
                        false,
                        app,
                    )
                    .with_navigation(
                        host.target.label().to_owned(),
                        WorkspaceAction::ActivateMuxHost(host.id),
                        WorkspaceAction::CloseMuxHost(host.id),
                    )
                    .build()
                    .finish(),
                );
            }
        }
        bar.add_child(
            self.render_tab_bar_icon_button(
                Appearance::as_ref(app),
                Icon::Plus,
                &self.mouse_states.new_tab_button,
                WorkspaceAction::ShowMuxNewHost,
                "New host tab".into(),
                None,
                false,
                false,
            )
            .finish(),
        );
    }

    pub(super) fn render_mux_layout(
        &self,
        content: Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let state = &self.mux_panel;
        let Some(nav) = &state.navigation else {
            return content;
        };
        let host = nav.selected();
        let discovery = state.discovery.get(&host.id);
        let mut header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Expanded::new(
                    1.,
                    mux_text("Windows", appearance.ui_font_size(), false, appearance),
                )
                .finish(),
            );
        header.add_child(self.mux_button(
            "files",
            button::Content::Icon(Icon::Folder),
            WorkspaceAction::ShowMuxFiles,
            false,
            false,
            appearance,
        ));
        header.add_child(self.mux_button(
            "new-window",
            button::Content::Icon(Icon::Plus),
            WorkspaceAction::ShowMuxNewWindow,
            false,
            discovery.is_some_and(|state| state.loading || state.error.is_some()),
            appearance,
        ));
        header.add_child(self.mux_button(
            "refresh",
            button::Content::Icon(Icon::Refresh),
            WorkspaceAction::RefreshMuxHost(host.id),
            false,
            discovery.is_some_and(|state| state.loading),
            appearance,
        ));
        let mut list = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(3.);
        if let Some(discovery) = discovery {
            if discovery.loading {
                list.add_child(mux_note("Discovering windows…", appearance));
            }
            for session in &discovery.sessions {
                let active = host
                    .sessions
                    .get(&session.name)
                    .is_some_and(|native| *native == host.active);
                list.add_child(self.mux_row(
                    &format!("window:{}:{}", host.id, session.name),
                    &session.name,
                    Icon::Terminal,
                    WorkspaceAction::OpenMuxSession {
                        host: host.id,
                        session: session.clone(),
                    },
                    active,
                    appearance,
                ));
            }
            if let Some(error) = &discovery.error {
                list.add_child(mux_note(error, appearance));
            }
            if !discovery.loading && discovery.sessions.is_empty() {
                list.add_child(mux_note(
                    if discovery.error.is_some() {
                        "Could not load windows"
                    } else {
                        "No zmx windows yet"
                    },
                    appearance,
                ));
            }
        }
        let scroll = ClippedScrollable::vertical(
            state.session_scroll.clone(),
            list.finish(),
            ScrollbarWidth::Custom(4.),
            appearance.theme().nonactive_ui_detail().into(),
            appearance.theme().active_ui_detail().into(),
            warpui::elements::Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        let sidebar = ConstrainedBox::new(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_child(header.finish())
                    .with_child(Shrinkable::new(1., scroll).finish())
                    .finish(),
            )
            .with_padding(Padding::uniform(8.))
            .with_background(appearance.theme().background())
            .with_border(Border::right(1.).with_border_fill(appearance.theme().outline()))
            .finish(),
        )
        .with_width(224.)
        .finish();
        let has_window = host.sessions.values().any(|native| *native == host.active);
        let terminal = if has_window
            || matches!(host.target, Target::Local)
            || discovery.is_some_and(|state| state.ssh_terminal_open && host.active == host.anchor)
        {
            content
        } else {
            let mut empty = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(16.);
            empty.add_child(mux_text(host.target.label(), 20., true, appearance));
            let message = discovery
                .and_then(|state| state.error.as_deref())
                .unwrap_or(if discovery.is_some_and(|state| state.loading) {
                    "Connecting and discovering zmx windows…"
                } else {
                    "Select a window on the left, or create a new zmx window."
                });
            empty.add_child(
                ConstrainedBox::new(mux_note(message, appearance))
                    .with_max_width(460.)
                    .finish(),
            );
            empty.add_child(self.mux_button(
                "empty-new-window",
                button::Content::IconAndLabel(Icon::Plus, "New zmx window".into()),
                WorkspaceAction::ShowMuxNewWindow,
                true,
                discovery.is_some_and(|state| state.loading || state.error.is_some()),
                appearance,
            ));
            empty.add_child(self.mux_button(
                "authenticate",
                button::Content::IconAndLabel(Icon::Terminal, "Open SSH terminal".into()),
                WorkspaceAction::AuthenticateMuxHost(host.id),
                false,
                false,
                appearance,
            ));
            Align::new(empty.finish()).finish()
        };
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(sidebar)
            .with_child(Expanded::new(1., terminal).finish())
            .finish()
    }

    pub(super) fn render_mux_dialog(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let state = &self.mux_panel;
        let dialog = state.dialog?;
        if dialog == MuxDialog::Files {
            return Some(self.render_mux_files(app));
        }
        let window_name_exists = state
            .navigation
            .as_ref()
            .and_then(|nav| state.discovery.get(&nav.selected))
            .is_some_and(|discovery| {
                discovery
                    .sessions
                    .iter()
                    .any(|session| session.name == state.window_name.trim())
            });
        let appearance = Appearance::as_ref(app);
        let (title, subtitle) = match dialog {
            MuxDialog::Files => unreachable!(),
            MuxDialog::NewHost => (
                "New host tab",
                "Open a local workspace or connect to a saved SSH host.",
            ),
            MuxDialog::ManageHosts => (
                "Saved SSH hosts",
                "Only hosts you add here appear when creating a remote tab.",
            ),
            MuxDialog::ImportHosts => (
                "Add from SSH config",
                "Select aliases to add to your saved hosts. SSH config is never modified.",
            ),
            MuxDialog::NewWindow => (
                "New zmx window",
                "Create a persistent terminal on the selected host.",
            ),
        };
        let mut heading = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(1., mux_text(title, 18., true, appearance)).finish());
        heading.add_child(self.mux_button(
            "dismiss",
            button::Content::Icon(Icon::X),
            WorkspaceAction::DismissMuxDialog,
            false,
            false,
            appearance,
        ));
        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.);
        if let Some(error) = &state.registry_error {
            body.add_child(mux_note(error, appearance));
        }
        if let Some(error) = &state.import_error {
            body.add_child(mux_note(error, appearance));
        }
        match dialog {
            MuxDialog::Files => unreachable!(),
            MuxDialog::NewWindow => {
                if let Some(editor) = &state.window_name_editor {
                    body.add_child(
                        TextInput::new(editor.clone(), UiComponentStyles::default())
                            .build()
                            .finish(),
                    );
                }
                if !state.window_name.is_empty() && !valid_session_name(state.window_name.trim()) {
                    body.add_child(mux_note(
                        "Use a name without control characters or a leading dash.",
                        appearance,
                    ));
                }
                if window_name_exists {
                    body.add_child(mux_note(
                        "A window with this name already exists.",
                        appearance,
                    ));
                }
            }
            MuxDialog::NewHost => {
                body.add_child(self.mux_row(
                    "local",
                    "Local terminal",
                    Icon::Terminal,
                    WorkspaceAction::CreateMuxHost(None),
                    false,
                    appearance,
                ));
                body.add_child(mux_note("REMOTE SSH", appearance));
                if !state.registry_ready {
                    body.add_child(mux_note("Loading saved hosts…", appearance));
                } else if state.saved.aliases.is_empty() {
                    body.add_child(mux_note(
                        "No saved hosts. Add a host to connect remotely.",
                        appearance,
                    ));
                }
                for alias in &state.saved.aliases {
                    body.add_child(self.mux_row(
                        &format!("connect:{alias}"),
                        alias,
                        Icon::Globe,
                        WorkspaceAction::CreateMuxHost(Some(alias.clone())),
                        false,
                        appearance,
                    ));
                }
            }
            MuxDialog::ManageHosts => {
                if state.saved.aliases.is_empty() {
                    body.add_child(mux_note("You have not added any SSH hosts.", appearance));
                }
                for alias in &state.saved.aliases {
                    body.add_child(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_child(
                                Expanded::new(1., mux_text(alias, 13., false, appearance)).finish(),
                            )
                            .with_child(self.mux_button(
                                &format!("remove:{alias}"),
                                button::Content::Label("Remove".into()),
                                WorkspaceAction::RemoveMuxSavedHost(alias.clone()),
                                false,
                                false,
                                appearance,
                            ))
                            .finish(),
                    );
                }
            }
            MuxDialog::ImportHosts => {
                if state.importing {
                    body.add_child(mux_note("Reading ~/.ssh/config…", appearance));
                } else if state.candidates.is_empty() {
                    body.add_child(mux_note(
                        "No concrete Host aliases found in ~/.ssh/config.",
                        appearance,
                    ));
                }
                for alias in &state.candidates {
                    let added = state.saved.aliases.contains(alias);
                    let selected = state.selected_candidates.contains(alias);
                    if added {
                        body.add_child(mux_note(&format!("{alias} · Already added"), appearance));
                    } else {
                        body.add_child(self.mux_row(
                            &format!("candidate:{alias}"),
                            alias,
                            if selected {
                                Icon::CheckCircleBroken
                            } else {
                                Icon::Circle
                            },
                            WorkspaceAction::ToggleMuxCandidate(alias.clone()),
                            selected,
                            appearance,
                        ));
                    }
                }
            }
        }
        let scroll = ClippedScrollable::vertical(
            state.dialog_scroll.clone(),
            body.finish(),
            ScrollbarWidth::Custom(4.),
            appearance.theme().nonactive_ui_detail().into(),
            appearance.theme().active_ui_detail().into(),
            warpui::elements::Fill::None,
        )
        .with_overlayed_scrollbar()
        .finish();
        let mut footer = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_spacing(8.);
        match dialog {
            MuxDialog::Files => unreachable!(),
            MuxDialog::NewWindow => {
                footer.add_child(self.mux_button(
                    "cancel-window",
                    button::Content::Label("Cancel".into()),
                    WorkspaceAction::DismissMuxDialog,
                    false,
                    false,
                    appearance,
                ));
                let disabled = !valid_session_name(state.window_name.trim()) || window_name_exists;
                footer.add_child(self.mux_button(
                    "create-window",
                    button::Content::Label("Create window".into()),
                    WorkspaceAction::CreateMuxWindow,
                    true,
                    disabled,
                    appearance,
                ));
            }
            MuxDialog::NewHost => footer.add_child(self.mux_button(
                "manage",
                button::Content::Label("Manage SSH hosts…".into()),
                WorkspaceAction::ShowMuxManageHosts,
                false,
                !state.registry_ready,
                appearance,
            )),
            MuxDialog::ManageHosts => {
                footer.add_child(self.mux_button(
                    "back-new",
                    button::Content::Label("Back".into()),
                    WorkspaceAction::ShowMuxNewHost,
                    false,
                    false,
                    appearance,
                ));
                footer.add_child(self.mux_button(
                    "import",
                    button::Content::IconAndLabel(Icon::Plus, "Add from SSH config".into()),
                    WorkspaceAction::ImportMuxHosts,
                    true,
                    state.registry_error.is_some(),
                    appearance,
                ));
            }
            MuxDialog::ImportHosts => {
                footer.add_child(self.mux_button(
                    "cancel-import",
                    button::Content::Label("Cancel".into()),
                    WorkspaceAction::ShowMuxManageHosts,
                    false,
                    false,
                    appearance,
                ));
                footer.add_child(self.mux_button(
                    "save-import",
                    button::Content::Label(
                        format!("Add {} hosts", state.selected_candidates.len()).into(),
                    ),
                    WorkspaceAction::SaveMuxSelectedHosts,
                    true,
                    state.selected_candidates.is_empty() || state.importing,
                    appearance,
                ));
            }
        }
        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(16.)
            .with_child(heading.finish())
            .with_child(mux_note(subtitle, appearance))
            .with_child(ConstrainedBox::new(scroll).with_max_height(340.).finish())
            .with_child(footer.finish())
            .finish();
        let modal = ConstrainedBox::new(
            Container::new(column)
                .with_padding(Padding::uniform(24.))
                .with_background(appearance.theme().surface_3())
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(12.)))
                .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
                .finish(),
        )
        .with_width(520.)
        .finish();
        Some(
            Dismiss::new(Align::new(modal).finish())
                .prevent_interaction_with_other_elements()
                .on_dismiss(|ctx, _| ctx.dispatch_typed_action(WorkspaceAction::DismissMuxDialog))
                .finish(),
        )
    }

    fn mux_button(
        &self,
        id: &str,
        content: button::Content,
        action: WorkspaceAction,
        primary: bool,
        disabled: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let mut buttons = self.mux_panel.buttons.borrow_mut();
        let component = buttons.entry(id.to_owned()).or_default();
        let theme: &dyn button::Theme = if primary {
            &button::themes::Primary
        } else {
            &button::themes::Naked
        };
        let mut options = button::Options::default(appearance);
        options.disabled = disabled;
        options.on_click = Some(Box::new(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone())
        }));
        component.render(
            appearance,
            button::Params {
                content,
                theme,
                options,
            },
        )
    }

    fn mux_row(
        &self,
        id: &str,
        label: &str,
        icon: Icon,
        action: WorkspaceAction,
        selected: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let mouse = self
            .mux_panel
            .rows
            .borrow_mut()
            .entry(id.to_owned())
            .or_default()
            .clone();
        let theme = appearance.theme();
        let label = label.to_owned();
        Hoverable::new(mouse, move |hover| {
            let color = theme.main_text_color(theme.background());
            let row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(
                    ConstrainedBox::new(icon.to_warpui_icon(color).finish())
                        .with_width(16.)
                        .with_height(16.)
                        .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        Text::new_inline(
                            label,
                            appearance.ui_font_family(),
                            appearance.ui_font_size(),
                        )
                        .with_color(color.into())
                        .with_clip(ClipConfig::ellipsis())
                        .finish(),
                    )
                    .finish(),
                )
                .finish();
            let mut container = Container::new(warpui::elements::Clipped::new(row).finish())
                .with_padding(Padding::uniform(8.))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                    super::vertical_tabs::ROW_CORNER_RADIUS,
                )));
            if let Some(background) = super::vertical_tabs::pane_row_background(
                None,
                selected,
                false,
                hover.is_hovered(),
                false,
                theme,
            ) {
                container = container.with_background(background);
            }
            container = container.with_border(Border::all(1.).with_border_fill(if selected {
                internal_colors::fg_overlay_3(theme).into()
            } else {
                warpui::elements::Fill::None
            }));
            container.finish()
        })
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .with_cursor(Cursor::PointingHand)
        .finish()
    }
}

fn mux_text(text: &str, size: f32, bold: bool, appearance: &Appearance) -> Box<dyn Element> {
    Text::new(text.to_owned(), appearance.ui_font_family(), size)
        .with_color(
            appearance
                .theme()
                .main_text_color(appearance.theme().background())
                .into(),
        )
        .with_style(Properties {
            weight: if bold {
                Weight::Semibold
            } else {
                Weight::Normal
            },
            ..Default::default()
        })
        .finish()
}
fn mux_note(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Text::new(text.to_owned(), appearance.ui_font_family(), 12.)
        .with_color(
            appearance
                .theme()
                .sub_text_color(appearance.theme().background())
                .into(),
        )
        .finish()
}
