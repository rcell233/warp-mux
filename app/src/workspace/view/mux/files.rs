use warpui::elements::Percentage;
use warpui::platform::SaveFilePickerConfiguration;

use super::*;
use crate::view_components::DismissibleToast;
use crate::workspace::mux::files::{self, Listing};

#[derive(Default)]
pub(super) struct FileBrowser {
    target: Option<Target>,
    editor: Option<ViewHandle<EditorView>>,
    base: String,
    listing: Option<Listing>,
    loading: bool,
    downloading: bool,
    choosing_destination: bool,
    error: Option<String>,
    generation: u64,
    scroll: ClippedScrollStateHandle,
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
                workspace.mux_panel.files.base = editor.as_ref(ctx).buffer_text(ctx);
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
        let key = target.ssh_alias().unwrap_or("");
        let base = SavedHosts::load(&registry_path())
            .ok()
            .and_then(|saved| saved.base_paths.get(key).cloned())
            .unwrap_or_else(|| "~".into());
        let files = &mut self.mux_panel.files;
        files.target = Some(target);
        files.base = base.clone();
        files.listing = None;
        files.error = None;
        if let Some(editor) = files.editor.clone() {
            editor.update(ctx, |view, ctx| view.set_buffer_text(&base, ctx));
        }
        self.show_mux_dialog(Some(MuxDialog::Files), ctx);
        self.load_mux_directory(String::new(), ctx);
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
        // Navigation uses the committed base; editing the field takes effect only on Go/Enter.
        let base = if path.is_empty() {
            files.base.clone()
        } else {
            files
                .listing
                .as_ref()
                .map(|list| list.base.clone())
                .unwrap_or_else(|| files.base.clone())
        };
        files.generation += 1;
        let generation = files.generation;
        files.loading = true;
        files.error = None;
        let key = target.ssh_alias().unwrap_or("").to_owned();
        ctx.spawn(
            async move { files::list(target.ssh_alias(), &base, &path).await },
            move |workspace, result, ctx| {
                let files = &mut workspace.mux_panel.files;
                if files.generation != generation {
                    return;
                }
                files.loading = false;
                match result {
                    Ok(listing) => {
                        let save_result = (|| -> anyhow::Result<()> {
                            let mut saved = SavedHosts::load(&registry_path())?;
                            saved.base_paths.insert(key, listing.base.clone());
                            saved.save(&registry_path())
                        })();
                        files.error = save_result
                            .err()
                            .map(|error| format!("Could not remember base path: {error:#}"));
                        files.listing = Some(listing);
                    }
                    Err(error) => files.error = Some(format!("{error:#}")),
                }
                ctx.notify();
            },
        );
        ctx.notify();
    }

    pub(in crate::workspace::view) fn download_mux_file(
        &mut self,
        source: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let files = &self.mux_panel.files;
        if files.downloading || files.choosing_destination {
            return;
        }
        let Some(target) = files.target.clone() else {
            return;
        };
        let Some(entry) = files.listing.as_ref().and_then(|listing| {
            listing
                .entries
                .iter()
                .find(|entry| entry.path == source && !entry.directory)
        }) else {
            return;
        };
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
                workspace.mux_panel.files.downloading = true;
                let source = source.clone();
                let target = target.clone();
                let destination = PathBuf::from(&path);
                ctx.spawn(
                    async move { files::download(target.ssh_alias(), &source, &destination).await },
                    move |workspace, result, ctx| {
                        workspace.mux_panel.files.downloading = false;
                        let toast = match result {
                            Ok(()) => DismissibleToast::success(format!("Downloaded to {path}")),
                            Err(error) => DismissibleToast::error(format!("{error:#}")),
                        };
                        workspace
                            .toast_stack
                            .update(ctx, |view, ctx| view.add_ephemeral_toast(toast, ctx));
                        ctx.notify();
                    },
                );
                ctx.notify();
            },
            config,
        );
    }

    pub(in crate::workspace::view) fn render_mux_files(
        &self,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let files = &self.mux_panel.files;
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
            files.loading,
            appearance,
        ));
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.)
            .with_child(heading)
            .with_child(base.finish());
        let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if let Some(listing) = &files.listing {
            let mut navigation = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.);
            navigation.add_child(self.mux_button(
                "file-up",
                button::Content::Label("↑".into()),
                WorkspaceAction::OpenMuxDirectory(listing.parent.clone().unwrap_or_default()),
                false,
                files.loading || listing.parent.is_none(),
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
                files.loading,
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
                            .with_child(self.mux_button(
                                &format!("download:{}", entry.path),
                                button::Content::IconAndLabel(Icon::Download, "Download".into()),
                                WorkspaceAction::DownloadMuxFile(entry.path.clone()),
                                false,
                                files.downloading || files.choosing_destination || files.loading,
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
        if files.loading {
            column.add_child(mux_note("Loading files…", appearance));
        }
        if files.downloading {
            column.add_child(mux_note(
                "Downloading… You can close this window while the transfer continues.",
                appearance,
            ));
        }
        if let Some(error) = &files.error {
            column.add_child(mux_note(error, appearance));
        }
        column.add_child(
            Shrinkable::new(
                1.,
                ClippedScrollable::vertical(
                    files.scroll.clone(),
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
        .on_dismiss(|ctx, _| ctx.dispatch_typed_action(WorkspaceAction::DismissMuxDialog))
        .finish()
    }
}
