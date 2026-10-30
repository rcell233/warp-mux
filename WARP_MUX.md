# Warp Mux — zmx workspaces

The macOS fork uses two simultaneous navigation levels:

| Surface | Contents | Selection behavior |
| --- | --- | --- |
| Top host strip | Local workspace and open SSH hosts | Restores that host's last selected terminal |
| Left window rail | Only the selected host's zmx sessions | Opens or selects its native Warp terminal |
| Main area | Selected terminal, connection/loading state, or actionable error | No persistent host directory |

## Connection flow

Click **+** in the native top tab bar (or Cmd+T) to open the connection chooser. Choose **Local terminal** or one
of your saved SSH hosts. The chooser closes when the host tab opens. Opening the same host again
selects its existing host tab. Host tabs and zmx windows are separate navigation levels;
horizontal and vertical tab layouts are no longer alternatives in this fork.

A remote host is discovered in the background; its first zmx window is selected when available.
Switching windows preserves the local SSH clients and uses Warp's native terminal renderer.
Use **+** in the window rail to create a named persistent zmx window, and Refresh to discover external changes. Same-named windows on different hosts are
independent. Closing a host tab disconnects its local clients; it never kills remote zmx sessions.

## Saved host management

From **+ → Manage SSH hosts**, choose **Add from SSH config**. Only this screen reads and
shows all concrete aliases in `~/.ssh/config` and ordinary Include files/globs. Select individual
aliases and click **Add hosts**. Already-added aliases are marked and cannot be selected twice.

The saved list is maintained separately at `mux/hosts.json` in Warp OSS's application data
directory. Add/remove changes are persisted atomically. Removing an entry does not modify SSH
config, disconnect an open host, or cause it to be automatically imported again. Opening the
chooser reloads the saved list so changes made in another window are visible. No credentials
or private-key contents are copied into the registry.

Host management, saved-host selection, and SSH config candidates appear only in these transient
screens. They are never permanent panels in the terminal workspace. Cancel, Escape, the close
button, or clicking outside dismisses the chooser without creating a host connection.

## SSH and zmx

Only **zmx** is supported by this feature. Local discovery is also supported. SSH hosts require a
POSIX `sh` and zmx on PATH (`~/.local/bin`, `~/bin`, `/opt/homebrew/bin`, and `/usr/local/bin` are
included). SSH itself resolves User, Port, ProxyJump, identities, and other config options.

Discovery is noninteractive with a 20-second timeout. If a host key or credentials require
interaction, use **Open SSH terminal**, authenticate, then Refresh. Background discovery needs
an agent/unlocked identity or a reusable SSH ControlMaster; an interactive password login alone
does not cache credentials for subsequent processes. Host-key verification is never disabled.

The UI calls zmx sessions “windows.” It does not invent a second multiplexer or emulate terminal
content in a web view. Session lists currently refresh on host creation and manually; external
session creation/removal is reflected after Refresh. Existing Warp split-pane functionality
remains inside the selected native terminal.

## Automatic SSH reconnection

Managed remote zmx terminals send SSH keepalives every 15 seconds and treat three unanswered
probes as a lost connection. A transport failure retries the same host/session with delays of
1, 2, 4, 8, 16, then at most 30 seconds. After a sleep/network interruption, recovery stays in the
existing tab and reattaches to the same zmx session. Closing the tab cancels the connection; Ctrl-C
can stop the retry wait. A normal detach/logout or remote-command error does not start retries.

The first connection permits interactive authentication. Retries use BatchMode, so automatic
recovery requires an unlocked key/SSH agent or reusable authenticated connection. Password-only
sessions may need manual authentication. Reconnection relies on the remote zmx session surviving;
it cannot recover processes lost to a remote reboot. Apply this build by reconnecting existing
host tabs once so their SSH clients use the new wrapper.

## File browser

The folder button in the window rail, Project Explorer toolbar entry, and its shortcut open a
floating file browser. It overlays the terminal instead of adding a docked panel. Only Escape
and the close button dismiss it; clicking blank space or outside the panel keeps it open. Other Warp tools retain their native toolbar.

Choose **Base path** and press Go/Enter. Valid base paths and the last successfully visited directory are remembered per SSH host (and
separately for Local) in the saved-host registry, including across app restarts. During the same
app session, reopening a browser restores its cached listing, input, and scroll position without
reloading. Use Refresh for remote changes. If the last directory no longer exists, Go opens the base path. The browser starts at that root, includes hidden files,
shows folders first, and lets you navigate subdirectories or return up as far as the base path.
Changing the input does not affect navigation until Go/Enter. Listings are capped at 2,000 entries.

Use **Download** beside a file and choose a local destination in the system Save dialog. Transfers
run independently of the terminal using OpenSSH SFTP and the host alias from SSH config. A failed
transfer leaves an existing destination intact. The modal can close while a download continues;
a toast reports its outcome. **Ctrl-click Download** saves to the system Downloads directory
without a picker and replaces an existing same-named file after the transfer succeeds. Ordinary
clicks retain the Save dialog. Directory downloads are not supported.

Listing requires `python3` on the selected host; remote downloads require its SFTP subsystem.
Background SSH/SFTP authentication uses your agent or reusable ControlMaster connection, just
like zmx discovery. This does not add a password store. The native Project Explorer in this
upstream revision does not support ordinary SSH sessions, so the floating browser supplies
that path using the saved SSH connection.

## Download

[Warp Mux macOS Actions](https://github.com/rcell233/warp-mux/actions/workflows/mux-macos.yml)
builds architecture-specific artifacts:

| Artifact | Mac |
| --- | --- |
| WarpMux-macOS-aarch64 | Apple Silicon |
| WarpMux-macOS-x86_64 | Intel |

Download an artifact from a successful run and unzip to obtain the DMG and SHA-256 checksum.
Install **WarpOss.app** alongside official Warp. The package is signed ad-hoc, without Apple
notarization; macOS may require allowing it in Privacy & Security. Artifacts remain available
for 14 days. No Apple credentials or Warp internal build services are required.

## Retrying an individual architecture

In Actions → Warp Mux macOS → Run workflow, choose `aarch64`, `x86_64`, or `both`.
The architecture choices have separate concurrency groups, so an ARM-only recovery build can
run while the Intel job from a full build is still active. Normal pushes still build both.
A recovery push can select one architecture with `[arm-only]` or `[intel-only]` in its commit
message. The cache keys remain architecture-specific and unchanged.

Apple Silicon builds use the standard `macos-15` ARM runner with Xcode 26; Intel builds use
`macos-26-intel`. The ARM runner was moved from `macos-26` after repeated failures to acquire
a hosted runner before any build steps ran. GitHub's [macOS 15 ARM image](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md)
includes Xcode 26. Switching images can change cache reuse and does not guarantee runner
availability. A runner-allocation failure has no compiler log; successful artifacts from the
other architecture remain downloadable even when the overall workflow is marked failed.

## Validation

```sh
cargo test --locked --manifest-path tools/mux-tests/Cargo.toml
cargo clippy --locked --manifest-path tools/mux-tests/Cargo.toml --all-targets -- -D warnings
./script/macos/bundle --channel oss --profile mux --arch aarch64 --check-only
./script/macos/bundle --channel oss --profile mux --selfsign --arch aarch64
```

The production backend is tested independently of the GPU stack. Tests cover registry
persistence/removal, malformed-file handling, host/window selection isolation, config Includes,
zmx output, and shell escaping. CI checks the complete GUI, builds DMGs, then verifies disk-image
integrity and app signatures. Real SSH authentication and mouse/keyboard behavior still require
Mac testing. Open host/window grouping currently lasts for the window lifetime; Warp's native
snapshot restore does not yet restore this grouping. Saved SSH hosts persist across restarts.
