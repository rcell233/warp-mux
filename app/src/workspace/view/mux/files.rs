use warpui::elements::{DispatchEventResult, EventHandler, Percentage};
use warpui::platform::SaveFilePickerConfiguration;

use super::*;
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
}

#[derive(Default)]
pub(super) struct FileBrowser {
    target: Option<Target>,
    editor: Option<ViewHandle<EditorView>>,
    states: HashMap<String, BrowserState>,
    downloading: bool,
    choosing_destination: bool,
}

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
        files.target = Some(target);
        let base = files.state().base.clone();
        if let Some(editor) = files.editor.clone() {
            editor.update(ctx, |view, ctx| view.set_buffer_text(&base, ctx));
        }
        self.show_mux_dialog(Some(MuxDialog::Files), ctx);
        if first_open {
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
        if files.downloading || files.choosing_destination {
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
        if direct {
            match dirs::download_dir()
                .and_then(|directory| files::download_destination(&directory, &entry.name).ok())
            {
                Some(destination) => self.start_mux_download(target, source, destination, ctx),
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
                workspace.start_mux_download(target, source, PathBuf::from(path), ctx);
            },
            config,
        );
    }

    fn start_mux_download(
        &mut self,
        target: Target,
        source: String,
        destination: PathBuf,
        ctx: &mut ViewContext<Self>,
    ) {
        self.mux_panel.files.downloading = true;
        let label = destination.display().to_string();
        ctx.spawn(
            async move {
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                files::download(target.ssh_alias(), &source, &destination).await
            },
            move |workspace, result, ctx| {
                workspace.mux_panel.files.downloading = false;
                let toast = match result {
                    Ok(()) => DismissibleToast::success(format!("Downloaded to {label}")),
                    Err(error) => DismissibleToast::error(format!("{error:#}")),
                };
                workspace
                    .toast_stack
                    .update(ctx, |view, ctx| view.add_ephemeral_toast(toast, ctx));
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
            .with_child(
                Expanded::new(
                    1.,
                    mux_text(&title, appearance.ui_font_size(), true, appearance),
                )
                .finish(),
            )
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
                                files.downloading || files.choosing_destination || state.loading,
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
            "Ctrl-click Download to save directly to Downloads and replace a same-named file.",
            appearance,
        ));
        if files.downloading {
            column.add_child(mux_note(
                "Downloading… You can close this window while the transfer continues.",
                appearance,
            ));
        }
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
        let panel = ConstrainedBox::new(
            Container::new(column.finish())
                .with_padding(Padding::uniform(16.))
                .with_background(appearance.theme().surface_1())
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
                .finish(),
        )
        .with_max_width(760.)
        .with_max_height(560.)
        .finish();
        Dismiss::new(
            Align::new(Percentage::width(0.85, Percentage::height(0.85, panel).finish()).finish())
                .finish(),
        )
        .prevent_interaction_with_other_elements()
        .on_dismiss(|_, _| {})
        .finish()
    }
}
