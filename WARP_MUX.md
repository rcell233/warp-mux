# Warp Mux — macOS test build

This fork adds an SSH host and zmx/tmux session browser to Warp's native vertical tab bar.
The app is currently packaged as **WarpOss.app** (bundle ID `dev.warp.WarpOss`) and can coexist
with the official Warp app. Existing Warp OSS installations share its preferences and app data.

## Download a test build

Open [Warp Mux macOS Actions](https://github.com/rcell233/warp-mux/actions/workflows/mux-macos.yml),
select a successful run, then download an artifact at the bottom of the run:

| Artifact | Mac |
| --- | --- |
| WarpMux-macOS-aarch64 | Apple Silicon (M series) |
| WarpMux-macOS-x86_64 | Intel |

Unzip the artifact, open the DMG, and drag WarpOss into Applications. The accompanying `.sha256`
file identifies the image checksum. Artifacts are retained for 14 days. These are test builds
signed ad-hoc, without Apple notarization; macOS may require allowing the app in
System Settings → Privacy & Security after attempting to open it.

The workflow runs on pushes to `master` and `codex/ssh-mux-macos`. After it is merged into the
default branch it can also be started from **Run workflow**. If GitHub has disabled Actions for
the fork, open the Actions tab and enable workflows first. No Warp internal services, Apple
certificates, or repository secrets are needed. First builds may take several hours; later
builds reuse a Rust dependency cache. Outputs are Actions artifacts, not GitHub Releases.

## Use

1. Configure aliases in `~/.ssh/config`, for example:

   ```sshconfig
   Host dev
       HostName example.com
       User me
       IdentityFile ~/.ssh/id_ed25519
   ```

2. Open WarpOss. Vertical tabs default on for new macOS preferences. If an existing preference
   keeps horizontal tabs, enable vertical tabs in Settings → Appearance.
3. Click **Import / reload ~/.ssh/config**. Import lists concrete `Host` aliases, including
   ordinary `Include` files/globs. Wildcards and negated host patterns are not offered as hosts.
   Add or remove aliases by editing SSH config and importing again; no SSH config is rewritten.
4. Expand a host to discover remote zmx and tmux sessions. If discovery cannot authenticate or
   the host key is unknown, click **Open SSH (authenticate)**, finish authentication, then refresh.
   Password authentication requires an SSH agent or a reusable ControlMaster for background
   discovery; an interactive login alone does not cache passwords for later SSH processes.
5. Click a session to attach in a real Warp terminal tab. Click it again to select the existing
   tab in that window. **Refresh sessions** discovers changes on the host.

Open tabs use Warp's existing switching, resizing, renaming, and closing behavior. Discovery
is lazy: listing a host does not open a connection to every session. Closing a tab disconnects
its client; there is no kill-session operation. The remote multiplexer controls persistence.

## Current limits

- macOS integration only. Remote hosts need a POSIX `sh` and zmx and/or tmux already installed.
- One remote **session** maps to one native Warp tab. tmux windows/panes remain inside tmux.
  Different tmux clients attached to the same session normally share the current window.
- Import enumerates SSH config aliases; OpenSSH itself resolves `User`, `Port`, `ProxyJump`,
  identities, and connection options. Conditional `Match` includes are skipped during import
  (except `Match all`); `Match exec` is not executed by the importer. Other conditional
  `Host` includes may offer aliases unavailable for a particular destination.
- Background discovery is noninteractive, has a 20-second timeout per backend, and never
  disables host-key verification. Host commands are only invoked when you expand or refresh.
- Remote tool lookup adds `~/.local/bin`, `~/bin`, `/opt/homebrew/bin`, and `/usr/local/bin` to
  the login shell PATH. Custom multiplexer socket paths and executable paths have no UI yet.
- Session labels reject terminal control characters. Names and SSH destinations are escaped
  independently for local and remote shells. tmux targets use stable session IDs.
- Session lists refresh manually; no live remote event subscription. Native-tab deduplication
  is scoped to the current window/lifetime. Restored or moved tabs are not rediscovered as
  existing connections. If SSH exits, close that tab and select the session again to reconnect.
- Local zmx/tmux browsing, creating/renaming/killing sessions, favorites, and per-window tmux
  mappings are not implemented in this first version.

## Development and checks

```sh
cargo test --locked --manifest-path tools/mux-tests/Cargo.toml
cargo clippy --locked --manifest-path tools/mux-tests/Cargo.toml --all-targets -- -D warnings
./script/run
./script/macos/bundle --channel oss --selfsign --arch aarch64
```

The small test harness imports production parser modules directly, avoiding a GUI/GPU link for
parser tests. The Actions builds compile the full GUI for both architectures, verify code
signatures and DMG integrity, then upload the image. They do not exercise SSH against your
hosts or prove the UI works on a physical Mac. Manual test checklist:

- Import aliases with `Include` globs; unknown host keys show an actionable discovery error.
- Authenticate with Open SSH, then list hosts with zmx only, tmux only, neither, and no sessions.
- Attach two sessions, switch repeatedly without extra tabs, close and reopen one session.
- Rename a native tab, refresh, and select its session again.
- Close Warp, reconnect from a new window, and confirm the remote job is still running.
- Exercise names with spaces, Unicode, quotes, and shell metacharacters.
