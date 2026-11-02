use futures::channel::oneshot;
use warp_terminal::model::escape_sequences::{BRACKETED_PASTE_END, BRACKETED_PASTE_START};
use warpui::clipboard::ClipboardContent;
use warpui::{SingletonEntity, ViewContext};

use super::TerminalView;
use crate::ai::agent_management::AgentNotificationsModel;
use crate::workspace::mux::{files, paste};

#[derive(Default)]
pub(super) struct RemoteImagePaste {
    binding: Option<(String, String)>,
    pub input_revision: u64,
    pending: usize,
    queued_bytes: usize,
    tail: Option<oneshot::Receiver<()>>,
}

impl TerminalView {
    pub(crate) fn bind_remote_image_paste(&mut self, host: Option<&str>, command: &str) {
        self.remote_image_paste.binding = host.map(|host| (host.to_owned(), command.to_owned()));
        self.remote_image_paste.input_revision =
            self.remote_image_paste.input_revision.wrapping_add(1);
    }

    fn remote_image_host(&self) -> Option<String> {
        let (host, command) = self.remote_image_paste.binding.as_ref()?;
        let model = self.model.lock();
        let block = model.block_list().active_block();
        (block.is_active_and_long_running()
            && block.command_with_secrets_unobfuscated(false).trim() == command.trim())
        .then(|| host.clone())
    }

    pub(super) fn paste_remote_clipboard_images(
        &mut self,
        clipboard: &ClipboardContent,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(host) = self.remote_image_host() else {
            return false;
        };
        if let Some(paths) = &clipboard.paths {
            let images = warpui::clipboard_utils::get_image_filepaths_from_paths(paths);
            if !images.is_empty() && images.len() == paths.len() {
                self.queue_remote_images(
                    host,
                    images
                        .into_iter()
                        .map(|p| paste::ImageSource::File(p.into()))
                        .collect(),
                    ctx,
                );
                return true;
            }
            if !paths.is_empty() {
                return false;
            }
        }
        let Some(images) = clipboard
            .images
            .as_ref()
            .filter(|images| !images.is_empty())
        else {
            return false;
        };
        let bytes = images.iter().map(|image| image.data.len()).sum::<usize>();
        if bytes > 64 * 1024 * 1024 || images.len() > 20 {
            self.show_error_toast("Paste at most 20 images and 64 MiB at a time.".into(), ctx);
            return true;
        }
        self.queue_remote_images(
            host,
            images
                .iter()
                .map(|image| paste::ImageSource::Clipboard(image.data.clone()))
                .collect(),
            ctx,
        );
        true
    }

    pub(super) fn paste_remote_image_files(
        &mut self,
        paths: &[String],
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let Some(host) = self.remote_image_host() else {
            return false;
        };
        self.queue_remote_images(
            host,
            paths
                .iter()
                .map(|path| paste::ImageSource::File(path.into()))
                .collect(),
            ctx,
        );
        true
    }

    fn queue_remote_images(
        &mut self,
        host: String,
        sources: Vec<paste::ImageSource>,
        ctx: &mut ViewContext<Self>,
    ) {
        let bytes = sources
            .iter()
            .map(paste::ImageSource::queued_bytes)
            .sum::<usize>();
        if self.remote_image_paste.pending + sources.len() > 20
            || self.remote_image_paste.queued_bytes + bytes > 64 * 1024 * 1024
        {
            self.show_error_toast(
                "Image upload queue is full. Wait for the pending uploads.".into(),
                ctx,
            );
            return;
        }
        for source in sources {
            self.queue_remote_image(host.clone(), source, ctx);
        }
    }

    fn queue_remote_image(
        &mut self,
        host: String,
        source: paste::ImageSource,
        ctx: &mut ViewContext<Self>,
    ) {
        let revision = self.remote_image_paste.input_revision;
        let block_index = self.model.lock().block_list().active_block_index();
        let byte_count = source.queued_bytes();
        self.remote_image_paste.pending += 1;
        self.remote_image_paste.queued_bytes += byte_count;
        let (done, next) = oneshot::channel();
        let previous = self.remote_image_paste.tail.replace(next);
        let terminal = ctx.handle();
        let terminal_id = self.view_id;
        let (tx, rx) = async_channel::bounded(1);
        AgentNotificationsModel::handle(ctx).update(ctx, |model, ctx| {
            let id = model.add_file_transfer(
                format!("Paste image · {host}"),
                "Queued".into(),
                true,
                terminal_id,
                ctx,
            );
            ctx.spawn_stream_local(
                crate::throttle::throttle(std::time::Duration::from_millis(250), rx),
                move |model, (bytes, total): (u64, u64), ctx| {
                    let fraction = if total == 0 {
                        0.
                    } else {
                        (bytes as f32 / total as f32).min(0.99)
                    };
                    model.update_file_transfer(
                        id,
                        format!("Uploading · {bytes} / {total} bytes"),
                        Some(fraction),
                        false,
                        ctx,
                    );
                },
                |_, _| {},
            );
            let upload_host = host.clone();
            ctx.spawn(
                async move {
                    if let Some(previous) = previous {
                        let _ = previous.await;
                    }
                    let _slot = files::TRANSFER_SLOTS.acquire().await?;
                    paste::upload_image(&upload_host, source, |bytes, total| {
                        let _ = tx.try_send((bytes, total));
                    })
                    .await
                },
                move |model, result: anyhow::Result<String>, ctx| {
                    let target = terminal.upgrade();
                    if let Some(target) = &target {
                        let _ = target.try_update(ctx, |view, _| {
                            view.remote_image_paste.pending =
                                view.remote_image_paste.pending.saturating_sub(1);
                            view.remote_image_paste.queued_bytes = view
                                .remote_image_paste
                                .queued_bytes
                                .saturating_sub(byte_count);
                        });
                    }
                    match result {
                        Ok(path) => {
                            let inserted = target.is_some_and(|target| {
                                target
                                    .try_update(ctx, |view, ctx| {
                                        if view.remote_image_paste.input_revision != revision
                                            || view.remote_image_host().as_deref()
                                                != Some(host.as_str())
                                            || !ctx.is_self_or_child_focused()
                                        {
                                            return false;
                                        }
                                        let ready = {
                                            let mut state = view.model.lock();
                                            state.block_list().active_block_index() == block_index
                                                && state.needs_bracketed_paste()
                                                && !state
                                                    .block_list()
                                                    .active_block()
                                                    .is_agent_in_control()
                                        };
                                        if !ready {
                                            return false;
                                        }
                                        let Ok(path_text) = paste::paste_path(&path) else {
                                            return false;
                                        };
                                        let mut bytes = BRACKETED_PASTE_START.to_vec();
                                        bytes.extend_from_slice(path_text.as_bytes());
                                        bytes.extend_from_slice(BRACKETED_PASTE_END);
                                        // Remote paths belong to one host; never broadcast them through synced inputs.
                                        view.write_to_pty(bytes, ctx);
                                        true
                                    })
                                    .unwrap_or(false)
                            });
                            let status = if inserted {
                                "Pasted into terminal"
                            } else {
                                "Uploaded · Copy path to paste when ready"
                            };
                            model.update_file_transfer(
                                id,
                                format!("{status}\n{path}"),
                                None,
                                false,
                                ctx,
                            );
                            model.set_transfer_path(
                                id,
                                paste::paste_path(&path).unwrap_or(path),
                                ctx,
                            );
                        }
                        Err(error) => model.update_file_transfer(
                            id,
                            format!("Image upload failed: {error:#}"),
                            None,
                            true,
                            ctx,
                        ),
                    }
                    // The next upload waits for delivery, not just transfer completion.
                    let _ = done.send(());
                },
            );
        });
    }
}
