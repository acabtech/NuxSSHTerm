# NuxSSHTerm

A native, tabbed SSH solution for Linux — a faithful reimplementation of
[WinSSHTerm](https://github.com/SmartBear/WinSSHTerm), built to replace the
Windows SSH workflow entirely on **Omarchy Quattro**. No Wine, no Windows
emulation: it uses the system OpenSSH client over a PTY.

> WinSSHTerm on Windows is really PuTTY (terminal) + WinSCP (SFTP) + Pageant (keys)
> + KeePass (vault) glued together. NuxSSHTerm rebuilds that same idea natively on
> Linux.

## Status

**v0.4 — Key manager milestone.** The shell/parity UI is in place (created
in v0.1), the encrypted password vault shipped in v0.2.0, the full settings
import story (PuTTY `.reg`/KiTTY `.txt`/WinSSHTerm XML/KeePass `.kdbx`, plus
`.ppk` key conversion) in v0.2.1, the dual-pane **SFTP commander**
(WinSCP-style, F5/F6/F7/F8) in v0.3.0, and the Pageant-equivalent **SSH key
manager** (detect/spawn a dedicated `ssh-agent`, list/add/remove keys,
per-session ForwardAgent) in v0.4.0.

## What's here (v0.4)

- **Tabbed terminal workspace** — multiple SSH sessions in tabs, with the
  WinSSHTerm-style window chrome (menu bar, quick strip, status bar).
- **Managed session tree** — a Connections panel with folders, add / duplicate /
  delete / reorder, expand & collapse, and a right-click context menu.
- **PTY + SSH sessions** — sessions launch through the system `ssh` over a
  `portable-pty` PTY; xterm.js renders the terminal. X11 forwarding and proxy
  settings map onto OpenSSH options.
- **Configuration panel** — edit connection attributes (host, port, user, key,
  X11, proxy, login commands) per host.
- **Encrypted password vault** — Argon2id + AES-256-GCM `vault.bin` under
  `~/.config/nuxsshterm/`; first-run wizard, unlock on start, lock/reset;
  passwords are never written to `connections.xml`.
- **Settings import wizard** — File → Import… scans PuTTY `.reg` (UTF-16LE),
  KiTTY `.txt`, WinSSHTerm `connections.xml`/`.settings`, and KeePass `.kdbx`;
  previews sessions with per-item mapping, warns on Windows key paths, converts
  `.ppk` keys via `puttygen` (originals untouched), then merges into the tree.
- **Settings export** — write the tree back out in WinSSHTerm
  `connections.xml` format (schema verified against `Migrate2WinSSHTerm`
  v0.23), so an existing Windows setup can be carried across.
- **SFTP commander (Copy Files)** — right-click a session → **Copy Files** (or
  Navigate → Copy Files) opens a WinSCP-style dual-pane commander: local pane +
  remote pane, path bars, and the WinSSHTerm shortcut set
  (F5 copy · F6 move · F7 mkdir · F8 delete · F9 properties · Ctrl+U swap
  panes · Ctrl+R refresh · Ctrl+T terminal to host). One persistent `sftp`
  child per tab (sequential scripted transfers); vault passwords are injected
  via an `SSH_ASKPASS` helper.
- **SSH key manager (Pageant equivalent)** — Tools → SSH key manager… adopts
  an existing `SSH_AUTH_SOCK` or spawns a dedicated `ssh-agent` (record kept
  in `settings.json`), lists/adds/removes keys via `ssh-add`, handles
  passphrase-protected keys through an askpass helper, and exports the socket
  to every terminal/SFTP child session. A per-session **Forward Agent**
  checkbox maps to `-o ForwardAgent=yes`.
- **Local persistence** — session tree stored under
  `~/.config/nuxsshterm/` in the native WinSSHTerm XML format; UI-local
  settings (agent record, ForwardAgent flags) in `settings.json`.

## Screenshots

Development screenshots are in [`docs/screenshots/`](docs/screenshots/):

- `v0.1-shell.png` — the shell UI
- `v0.1-config-panel.png` — the connection configuration panel
- `v0.1-ssh-session.png` — an active SSH session

## Requirements

- Linux (targeted for Omarchy; also builds `.deb` for
  Debian/Ubuntu and .rpm for RedHat)
- Node.js 18+ and npm
- Rust (stable) and Cargo
- The usual Tauri 2 Linux prerequisites (WebKitGTK, etc.):
  `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`

## Building

```bash
# Install JS deps
npm install

# Dev mode (Vite + Tauri, hot reload)
npm run tauri dev

# Type-check + production build of the web frontend
npm run build

# Build native bundles (deb + AppImage)
npm run tauri build
```

Bundles are written to `src-tauri/target/release/bundle/`:
- `.deb` for Debian/Ubuntu
- `.AppImage` for Arch / any distro

## Configuration & data

- `~/.config/nuxsshterm/` — session tree (WinSSHTerm `connections.xml`
  format), the SFTP askpass helper, and the encrypted vault; **passwords are
  never written to `connections.xml`** — they live in the vault while unlocked
  and only in memory when locked.

## Roadmap (upcoming)

- **SSH key manager (Pageant equivalent)** — detect/adopt `SSH_AUTH_SOCK` or spawn a
  dedicated `ssh-agent`; list/add/remove keys; per-session ForwardAgent (v0.4.0).
- **Quick-launch bar, tray icon, session reconnect, system theme**, and further
  WinSSHTerm parity.
- **In-process SSH (russh + russh-sftp)** — byte-level transfer progress,
  resume/parallel transfers, password auth without the askpass helper.

## Project docs

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — architecture, data model, and
  WinSSHTerm schema notes.
- [`docs/UI-SPEC.md`](docs/UI-SPEC.md) — UI parity spec derived from the
  WinSSHTerm screenshot.

## License

This project is licensed under the GPL v3 License - see the LICENSE file for details.