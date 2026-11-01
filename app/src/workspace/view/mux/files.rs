use warpui::elements::Percentage;
use warpui::platform::SaveFilePickerConfiguration;

use super::*;
use crate::ai::agent_management::AgentNotificationsModel;
use crate::view_components::DismissibleToast;
use crate::workspace::mux::files::{self, Listing};

#[derive(Default)]
struct BrowserState {
    base: String,
    listing: Option<Listing>,
    loading: bool,
    error: Option<String>,
    generation: u64,
    scroll: ClippedScrollStateHandle,
    dirty: bool,
}

#[derive(Default)]
pub(super) struct FileBrowser {
    target: Option<Target>,
    editor: Option<ViewHandle<EditorView>>,
    states: HashMap<String, BrowserState>,
    choosing_destination: bool,
    choosing_upload: bool,
}

enum Transfer {
    Download {
        source: String,
        destination: PathBuf,
        total: u64,
    },
    Upload {
        source: PathBuf,
        base: String,
        directory: String,
    },
}

static TRANSFER_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);

impl FileBrowser {
    fn key(&self) -> String {
        self.target
            .as_ref()
            .and_then(Target::ssh_alias)
            .unwrap_or("")
            .to_owned()
    }
    fn state(&self) -> &BrowserState {
        &self.states[&self.key()]
    }
    fn state_mut(&mut self) -> &mut BrowserState {
        let key = self.key();
        self.states.entry(key).or_default()
    }
}

impl Workspace {
    pub(in crate::workspace::view) fn initialize_mux_files(&mut self, ctx: &mut ViewContext<Self>) {
        let editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(appearance.ui_font_size()), appearance),
                    select_all_on_focus: true,
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text("Base path, e.g. /home/me/projects", ctx);
            editor
        });
        ctx.subscribe_to_view(&editor, |workspace, editor, event, ctx| match event {
            EditorEvent::Edited(_) => {
                if workspace.mux_panel.files.target.is_some() {
                    workspace.mux_panel.files.state_mut().base =
                        editor.as_ref(ctx).buffer_text(ctx);
                }
                ctx.notify();
            }
            EditorEvent::Enter => workspace.apply_mux_base_path(ctx),
            EditorEvent::Escape => workspace.show_mux_dialog(None, ctx),
            _ => {}
        });
        self.mux_panel.files.editor = Some(editor);
    }

    pub(in crate::workspace::view) fn open_mux_files(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(target) = self
            .mux_panel
            .navigation
            .as_ref()
            .map(|nav| nav.selected().target.clone())
        else {
            return;
        };
        let key = target.ssh_alias().unwrap_or("").to_owned();
        let files = &mut self.mux_panel.files;
        let first_open = !files.states.contains_key(&key);
        let mut restore_path = String::new();
        let dirty = files.states.get(&key).is_some_and(|state| state.dirty);
        if first_open {
            let saved = SavedHosts::load(&registry_path()).unwrap_or_default();
            let base = saved
                .base_paths
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "~".into());
            restore_path = saved.last_paths.get(&key).cloned().unwrap_or_default();
            files.states.insert(
                key.clone(),
                BrowserState {
                    base,
                    ..Default::default()
                },
            );
        }
        if dirty {
            restore_path = files.states[&key]
                .listing
                .as_ref()
                .map(|listing| listing.path.clone())
                .unwrap_or_default();
        }
        files.target = Some(target);
        let base = files.state().base.clone();
        if let Some(editor) = files.editor.clone() {
            editor.update(ctx, |view, ctx| view.set_buffer_text(&base, ctx));
        }
        self.show_mux_dialog(Some(MuxDialog::Files), ctx);
        if first_open || dirty {
            self.load_mux_directory(restore_path, ctx);
        }
    }

    pub(in crate::workspace::view) fn apply_mux_base_path(&mut self, ctx: &mut ViewContext<Self>) {
        if self.mux_panel.dialog == Some(MuxDialog::Files) {
            self.load_mux_directory(String::new(), ctx);
        }
    }

    pub(in crate::workspace::view) fn load_mux_directory(
        &mut self,
        path: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.mux_panel.dialog != Some(MuxDialog::Files) {
            return;
        }
        let files = &mut self.mux_panel.files;
        let Some(target) = files.target.clone() else {
            return;
        };
        let key = files.key();
        let state = files.state_mut();
        // Edits take effect on Go/Enter; navigation uses the last successful base.
        let base = if path.is_empty() {
            state.base.clone()
        } else {
            state
                .listing
                .as_ref()
                .map(|list| list.base.clone())
                .unwrap_or_else(|| state.base.clone())
        };
        state.generation += 1;
        let generation = state.generation;
        state.loading = true;
        state.error = None;
        ctx.spawn(
            async move { files::list(target.ssh_alias(), &base, &path).await },
            move |workspace, result, ctx| {
                let files = &mut workspace.mux_panel.files;
                let state = files.states.entry(key.clone()).or_default();
                if state.generation != generation {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(listing) => {
                        state.dirty = false;
                        let save_result = (|| -> anyhow::Result<()> {
                            let mut saved = SavedHosts::load(&registry_path())?;
                            saved.base_paths.insert(key.clone(), listing.base.clone());
                            saved.last_paths.insert(key, listing.path.clone());
                            saved.save(&registry_path())
                        })();
                        state.error = save_result
                            .err()
                            .map(|error| format!("Could not remember location: {error:#}"));
                        state.listing = Some(listing);
                    }
                    Err(error) => {
                        state.error = Some(format!("{error:#}. Use Go to return to the base path."))
                    }
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    pub(in crate::workspace::view) fn download_mux_file(
        &mut self,
        source: String,
        direct: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let files = &self.mux_panel.files;
        if files.choosing_destination {
            return;
        }
        let Some(target) = files.target.clone() else {
            return;
        };
        let Some(entry) = files.state().listing.as_ref().and_then(|listing| {
            listing
                .entries
                .iter()
                .find(|entry| entry.path == source && !entry.directory)
        }) else {
            return;
        };
        let total = entry.size;
        if direct {
            match dirs::download_dir()
                .and_then(|directory| files::download_destination(&directory, &entry.name).ok())
            {
                Some(destination) => self.start_mux_transfer(
                    target,
                    Transfer::Download {
                        source,
                        destination,
                        total,
                    },
                    ctx,
                ),
                None => self.toast_stack.update(ctx, |view, ctx| {
                    view.add_ephemeral_toast(
                        DismissibleToast::error(
                            "Could not resolve the Downloads directory or file name.".into(),
                        ),
                        ctx,
                    )
                }),
            }
            return;
        }
        let mut config =
            SaveFilePickerConfiguration::new().with_default_filename(entry.name.clone());
        if let Some(directory) = dirs::download_dir() {
            config = config.with_default_directory(directory);
        }
        self.mux_panel.files.choosing_destination = true;
        ctx.open_save_file_picker(
            move |path: Option<String>, workspace, ctx| {
                workspace.mux_panel.files.choosing_destination = false;
                let Some(path) = path else {
                    ctx.notify();
                    return;
                };
                workspace.start_mux_transfer(
                    target,
                    Transfer::Download {
                        source,
                        destination: PathBuf::from(path),
                        total,
                    },
                    ctx,
                );
            },
            config,
        );
    }

    pub(in crate::workspace::view) fn pick_mux_upload(&mut self, ctx: &mut ViewContext<Self>) {
        if self.mux_panel.files.choosing_upload {
            return;
        }
        let Some((target, base, directory)) = self.mux_upload_target() else {
            return;
        };
        self.mux_panel.files.choosing_upload = true;
        let workspace = ctx.handle();
        ctx.open_file_picker(
            move |result, ctx| {
                if let Some(workspace) = workspace.upgrade(ctx) {
                    workspace.update(ctx, |workspace, ctx| {
                        workspace.mux_panel.files.choosing_upload = false;
                        match result {
                            Ok(paths) => {
                                workspace.start_mux_uploads(target, base, directory, paths, ctx)
                            }
                            Err(error) => workspace.toast_stack.update(ctx, |view, ctx| {
                                view.add_ephemeral_toast(
                                    DismissibleToast::error(error.to_string()),
                                    ctx,
                                )
                            }),
                        }
                        ctx.notify();
                    });
                }
            },
            warpui::platform::FilePickerConfiguration::new().allow_multi_select(),
        );
        ctx.notify();
    }

    fn mux_upload_target(&self) -> Option<(Target, String, String)> {
        if !self.mux_files_open() {
            return None;
        }
        let files = &self.mux_panel.files;
        if files.state().loading {
            return None;
        }
        let listing = files.state().listing.as_ref()?;
        Some((
            files.target.clone()?,
            listing.base.clone(),
            listing.path.clone(),
        ))
    }

    pub(in crate::workspace::view) fn drop_mux_uploads(
        &mut self,
        paths: Vec<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some((target, base, directory)) = self.mux_upload_target() {
            self.start_mux_uploads(target, base, directory, paths, ctx);
        }
    }

    fn start_mux_uploads(
        &mut self,
        target: Target,
        base: String,
        directory: String,
        paths: Vec<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        for source in paths {
            self.start_mux_transfer(
                target.clone(),
                Transfer::Upload {
                    source: PathBuf::from(source),
                    base: base.clone(),
                    directory: directory.clone(),
                },
                ctx,
            );
        }
    }

    fn start_mux_transfer(
        &mut self,
        target: Target,
        transfer: Transfer,
        ctx: &mut ViewContext<Self>,
    ) {
        let upload = matches!(transfer, Transfer::Upload { .. });
        let label = match &transfer {
            Transfer::Download { destination, .. } => destination.display().to_string(),
            Transfer::Upload {
                source, directory, ..
            } => format!(
                "{directory}/{}",
                source.file_name().unwrap_or_default().to_string_lossy()
            ),
        };
        let title = format!(
            "{} · {}",
            if upload { "Upload" } else { "Download" },
            target.label()
        );
        let terminal = self
            .active_tab_pane_group()
            .as_ref(ctx)
            .active_session_view(ctx)
            .map(|view| view.id())
            .unwrap_or(self.active_tab_pane_group().id());
        let id = AgentNotificationsModel::handle(ctx).update(ctx, |model, ctx| {
            model.add_file_transfer(title, format!("Queued · {label}"), upload, terminal, ctx)
        });
        let (tx, rx) = async_channel::bounded(1);
        let progress_label = label.clone();
        ctx.spawn_stream_local(
            crate::throttle::throttle(std::time::Duration::from_millis(250), rx),
            move |_, (bytes, total): (u64, u64), ctx| {
                let fraction = if total == 0 {
                    0.
                } else {
                    ((bytes as f64 / total as f64) as f32).min(0.99)
                };
                let message = format!(
                    "{:.0}% · {} / {} bytes · {progress_label}",
                    fraction * 100.,
                    bytes,
                    total
                );
                AgentNotificationsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.update_file_transfer(id, message, Some(fraction), false, ctx)
                });
            },
            |_, _| {},
        );
        let key = target.ssh_alias().unwrap_or("").to_owned();
        ctx.spawn(
            async move {
                let _slot = TRANSFER_SLOTS.acquire().await?;
                match transfer {
                    Transfer::Download {
                        source,
                        destination,
                        total,
                    } => {
                        files::download_with_progress(
                            target.ssh_alias(),
                            &source,
                            &destination,
                            |bytes| {
                                let _ = tx.try_send((bytes, total));
                            },
                        )
                        .await
                    }
                    Transfer::Upload {
                        source,
                        base,
                        directory,
                    } => {
                        files::upload(
                            target.ssh_alias(),
                            &source,
                            &base,
                            &directory,
                            |bytes, total| {
                                let _ = tx.try_send((bytes, total));
                            },
                        )
                        .await
                    }
                }
            },
            move |workspace, result, ctx| {
                let failed = result.is_err();
                let message = match result {
                    Ok(()) => format!("Completed · {label}"),
                    Err(error) => format!("Failed · {label}: {error:#}"),
                };
                AgentNotificationsModel::handle(ctx).update(ctx, |model, ctx| {
                    model.update_file_transfer(id, message, None, failed, ctx)
                });
                if upload && !failed {
                    if let Some(state) = workspace.mux_panel.files.states.get_mut(&key) {
                        state.dirty = true;
                    }
                    if workspace.mux_files_open() && workspace.mux_panel.files.key() == key {
                        let path = workspace
                            .mux_panel
                            .files
                            .state()
                            .listing
                            .as_ref()
                            .map(|listing| listing.path.clone())
                            .unwrap_or_default();
                        workspace.load_mux_directory(path, ctx);
                    }
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    fn mux_download_button(
        &self,
        source: &str,
        disabled: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let button = self.mux_button(
            &format!("download:{source}"),
            button::Content::IconAndLabel(Icon::Download, "Download".into()),
            WorkspaceAction::DownloadMuxFile(source.to_owned()),
            false,
            disabled,
            appearance,
        );
        let source = source.to_owned();
        EventHandler::new(button)
            .on_right_mouse_down(move |ctx, app, _, _| {
                if !disabled && crate::tab::TabShortcutModifierState::as_ref(app).is_control_held()
                {
                    ctx.dispatch_typed_action(WorkspaceAction::QuickDownloadMuxFile(
                        source.clone(),
                    ));
                    DispatchEventResult::StopPropagation
                } else {
                    DispatchEventResult::PropagateToParent
                }
            })
            .finish()
    }

    pub(in crate::workspace::view) fn render_mux_files(
        &self,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let files = &self.mux_panel.files;
        let state = files.state();
        let title = format!(
            "Files · {}",
            files.target.as_ref().map(Target::label).unwrap_or("Local")
        );
        let heading = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_child(
                Expanded::new(
                    1.,
                    Text::new_inline(
                        title,
                        appearance.ui_font_family(),
                        appearance.ui_font_size(),
                    )
                    .with_color(appearance.theme().foreground().into())
                    .with_clip(ClipConfig::ellipsis())
                    .finish(),
                )
                .finish(),
            )
            .with_child(self.mux_button(
                "upload-files",
                button::Content::Label("Upload…".into()),
                WorkspaceAction::PickMuxUpload,
                false,
                files.choosing_upload || state.loading || state.listing.is_none(),
                appearance,
            ))
            .with_child(self.mux_button(
                "file-transfers",
                button::Content::Label("Transfers".into()),
                WorkspaceAction::ShowMuxTransfers,
                false,
                false,
                appearance,
            ))
            .with_child(self.mux_button(
                "close-files",
                button::Content::Icon(Icon::X),
                WorkspaceAction::DismissMuxDialog,
                false,
                false,
                appearance,
            ))
            .finish();
        let mut base = Flex::row()
            .with_spacing(8.)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(mux_note("Base path", appearance));
        if let Some(editor) = &files.editor {
            base.add_child(
                Expanded::new(
                    1.,
                    TextInput::new(editor.clone(), UiComponentStyles::default())
                        .build()
                        .finish(),
                )
                .finish(),
            );
        }
        base.add_child(self.mux_button(
            "apply-base",
            button::Content::Label("Go".into()),
            WorkspaceAction::ApplyMuxBasePath,
            false,
            state.loading,
            appearance,
        ));
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.)
            .with_child(heading)
            .with_child(base.finish());
        let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if let Some(listing) = &state.listing {
            let mut navigation = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.);
            navigation.add_child(self.mux_button(
                "file-up",
                button::Content::Label("↑".into()),
                WorkspaceAction::OpenMuxDirectory(listing.parent.clone().unwrap_or_default()),
                false,
                state.loading || listing.parent.is_none(),
                appearance,
            ));
            navigation.add_child(
                Expanded::new(
                    1.,
                    Text::new_inline(
                        listing.path.clone(),
                        appearance.ui_font_family(),
                        appearance.ui_font_size(),
                    )
                    .with_color(appearance.theme().foreground().into())
                    .with_clip(ClipConfig::ellipsis())
                    .finish(),
                )
                .finish(),
            );
            navigation.add_child(self.mux_button(
                "file-refresh",
                button::Content::Icon(Icon::Refresh),
                WorkspaceAction::OpenMuxDirectory(listing.path.clone()),
                false,
                state.loading,
                appearance,
            ));
            column.add_child(navigation.finish());
            for entry in &listing.entries {
                if entry.directory {
                    rows.add_child(self.mux_row(
                        &format!("directory:{}", entry.path),
                        &entry.name,
                        Icon::Folder,
                        WorkspaceAction::OpenMuxDirectory(entry.path.clone()),
                        false,
                        appearance,
                    ));
                } else {
                    rows.add_child(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(8.)
                            .with_child(
                                Expanded::new(
                                    1.,
                                    Text::new_inline(
                                        entry.name.clone(),
                                        appearance.ui_font_family(),
                                        appearance.ui_font_size(),
                                    )
                                    .with_color(appearance.theme().foreground().into())
                                    .with_clip(ClipConfig::ellipsis())
                                    .finish(),
                                )
                                .finish(),
                            )
                            .with_child(mux_note(&format!("{} B", entry.size), appearance))
                            .with_child(self.mux_download_button(
                                &entry.path,
                                files.choosing_destination || state.loading,
                                appearance,
                            ))
                            .finish(),
                    );
                }
            }
            if listing.entries.is_empty() {
                rows.add_child(mux_note("This folder is empty.", appearance));
            }
            if listing.truncated {
                rows.add_child(mux_note(
                    "Showing the first 2,000 entries. Choose a narrower base path.",
                    appearance,
                ));
            }
        }
        if state.loading {
            column.add_child(mux_note("Loading files…", appearance));
        }
        column.add_child(mux_note(
            "Drop files here to upload to this folder. Uploads replace same-named files after success.",
            appearance,
        ));
        if let Some(error) = &state.error {
            column.add_child(mux_note(error, appearance));
        }
        column.add_child(
            Shrinkable::new(
                1.,
                ClippedScrollable::vertical(
                    state.scroll.clone(),
                    rows.finish(),
                    ScrollbarWidth::Custom(4.),
                    appearance.theme().nonactive_ui_detail().into(),
                    appearance.theme().active_ui_detail().into(),
                    warpui::elements::Fill::None,
                )
                .with_overlayed_scrollbar()
                .finish(),
            )
            .finish(),
        );
        let panel = super::file_drop::MuxFileDropTarget::new(
            ConstrainedBox::new(mux_dialog_surface(column.finish(), appearance))
                .with_max_width(760.)
                .with_max_height(560.)
                .finish(),
        )
        .finish();
        mux_dialog_overlay(
            Percentage::width(0.85, Percentage::height(0.85, panel).finish()).finish(),
        )
    }
}
