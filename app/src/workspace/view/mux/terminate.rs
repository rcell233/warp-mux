use super::*;

pub(super) struct PendingTermination {
    target: Target,
    session: Session,
    pub(super) busy: bool,
    error: Option<String>,
}

impl Workspace {
    pub(super) fn mux_session_is_terminating(&self, host: HostId, session: &Session) -> bool {
        self.mux_panel.termination.as_ref().is_some_and(|request| {
            request.busy
                && request.session == *session
                && self
                    .mux_panel
                    .navigation
                    .as_ref()
                    .and_then(|nav| nav.host(host))
                    .is_some_and(|group| group.target == request.target)
        })
    }

    pub(in crate::workspace) fn request_mux_termination(
        &mut self,
        host: HostId,
        session: &Session,
        ctx: &mut ViewContext<Self>,
    ) {
        if self
            .mux_panel
            .termination
            .as_ref()
            .is_some_and(|request| request.busy)
        {
            return;
        }
        let Some(target) = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.host(host))
            .map(|group| group.target.clone())
        else {
            return;
        };
        self.mux_panel.termination = Some(PendingTermination {
            target,
            session: session.clone(),
            busy: false,
            error: None,
        });
        self.show_mux_dialog(Some(MuxDialog::TerminateWindow), ctx);
    }

    pub(in crate::workspace) fn confirm_mux_termination(&mut self, ctx: &mut ViewContext<Self>) {
        if self.mux_panel.dialog != Some(MuxDialog::TerminateWindow) {
            return;
        }
        let Some(request) = self.mux_panel.termination.as_mut() else {
            return;
        };
        if request.busy {
            return;
        }
        if let Err(error) = mux::terminate::kill_command(&request.session) {
            request.error = Some(error.to_string());
            ctx.notify();
            return;
        }
        let Some(host) = self.mux_panel.navigation.as_ref().and_then(|nav| {
            nav.hosts
                .iter()
                .find(|group| group.target == request.target)
                .map(|group| group.id)
        }) else {
            request.error = Some("This host tab has closed. Reopen it to retry.".into());
            ctx.notify();
            return;
        };
        request.busy = true;
        request.error = None;
        let target = request.target.clone();
        let session = request.session.clone();
        let native = self
            .mux_panel
            .navigation
            .as_ref()
            .and_then(|nav| nav.host(host))
            .and_then(|group| group.sessions.get(&session.name))
            .copied();
        let selection = self.mux_panel.navigation.as_ref().map(|nav| {
            let selected = nav.selected();
            let next = if Some(selected.active) == native {
                selected
                    .sessions
                    .values()
                    .copied()
                    .find(|id| Some(*id) != native)
                    .unwrap_or(selected.anchor)
            } else {
                selected.active
            };
            (selected.id, next)
        });
        if let Some(native) = native
            && let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.pane_group.id() == native)
        {
            let pane_group = self.tabs[index].pane_group.clone();
            pane_group.update(ctx, |group, ctx| {
                group.for_all_terminal_panes(|view, ctx| view.cancel_mux_session(ctx), ctx);
            });
            self.remove_tab(index, false, true, ctx);
            if let Some((selected, next)) = selection {
                if let Some(index) = self.tabs.iter().position(|tab| tab.pane_group.id() == next) {
                    self.activate_tab(index, ctx);
                } else {
                    self.activate_mux_host(selected, ctx);
                }
            }
        }
        let state = self.mux_panel.discovery.entry(host).or_default();
        state.generation = state.generation.wrapping_add(1);
        state.loading = false;
        state.error = None;
        ctx.spawn(
            async move { mux::terminate::terminate(target.ssh_alias(), &session).await },
            |workspace, result, ctx| {
                let Some(mut request) = workspace.mux_panel.termination.take() else {
                    return;
                };
                request.busy = false;
                let host = workspace.mux_panel.navigation.as_ref().and_then(|nav| {
                    nav.hosts
                        .iter()
                        .find(|group| group.target == request.target)
                        .map(|group| group.id)
                });
                match result {
                    Ok(()) => {
                        if let Some(host) = host {
                            let state = workspace.mux_panel.discovery.entry(host).or_default();
                            state.generation = state.generation.wrapping_add(1);
                            state.loading = false;
                            state.sessions.retain(|session| *session != request.session);
                            workspace.refresh_mux_host(host, ctx);
                        }
                        if workspace.mux_panel.dialog == Some(MuxDialog::TerminateWindow) {
                            workspace.show_mux_dialog(None, ctx);
                        }
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        if let Some(host) = host {
                            let state = workspace.mux_panel.discovery.entry(host).or_default();
                            state.generation = state.generation.wrapping_add(1);
                            state.loading = false;
                            state.error = Some(error.clone());
                        }
                        request.error = Some(error);
                        if workspace.mux_panel.dialog == Some(MuxDialog::TerminateWindow) {
                            workspace.mux_panel.termination = Some(request);
                        }
                    }
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    pub(super) fn render_mux_termination(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let request = self.mux_panel.termination.as_ref()?;
        let appearance = Appearance::as_ref(app);
        let mut body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.)
            .with_child(mux_text("Terminate zmx window?", appearance.ui_font_size(), true, appearance))
            .with_child(mux_note(&format!("Host: {}", request.target.label()), appearance))
            .with_child(mux_note(&format!("Window: {}", request.session.name), appearance))
            .with_child(mux_note(
                "This will stop the terminal and its running processes, and disconnect all attached clients. This cannot be undone.",
                appearance,
            ));
        if let Some(error) = &request.error {
            body.add_child(mux_note(error, appearance));
        }
        let footer = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_spacing(8.)
            .with_child(self.mux_button(
                "cancel-terminate",
                button::Content::Label(if request.busy { "Hide" } else { "Cancel" }.into()),
                WorkspaceAction::DismissMuxDialog,
                false,
                false,
                appearance,
            ))
            .with_child(
                self.mux_button(
                    "confirm-terminate",
                    button::Content::Label(
                        if request.busy {
                            "Terminating…"
                        } else {
                            "Terminate window"
                        }
                        .into(),
                    ),
                    WorkspaceAction::ConfirmMuxTermination,
                    true,
                    request.busy,
                    appearance,
                ),
            )
            .finish();
        body.add_child(footer);
        Some(mux_dialog_overlay(
            ConstrainedBox::new(mux_dialog_surface(body.finish(), appearance))
                .with_width(480.)
                .finish(),
        ))
    }
}
