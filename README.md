# NuxSSHTerm

NuxSSHTerm is a native Linux SSH workspace inspired by [WinSSHTerm](https://github.com/WinSSHTerm/WinSSHTerm). It provides a tabbed terminal and file-transfer environment built with Tauri, React, and the system OpenSSH toolchain—no Wine, no Windows emulation.

## Highlights

- **Modern workspace UI** — dark-themed shell with menu bar, status bar, collapsible connections rail, and a dedicated Settings page.
- **Tileable tabs and panes** — open multiple terminals/commanders, drag tabs between panes, split the work area, and resize panes.
- **SSH sessions over a real PTY** — launches the system `ssh` client through `portable-pty`; terminal rendering is handled by xterm.js.
- **Session management** — folder-based connection tree with add/duplicate/delete, expand/collapse, and per-connection configuration.
- **WinSSHTerm-compatible storage/import/export** — reads and writes the WinSSHTerm `connections.xml` session format; imports PuTTY `.reg`, KiTTY `.txt`, WinSSHTerm XML/settings, and (best effort) KeePass `.kdbx`.
- **Encrypted password vault** — Argon2id + AES-256-GCM vault stored locally; passwords are kept out of exported/session XML.
- **SSH key manager** — adopts an existing `SSH_AUTH_SOCK` or manages a dedicated `ssh-agent`, with add/remove/list operations and optional per-session agent forwarding.
- **SFTP commander** — dual-pane local/remote file browser backed by the system `sftp` client for common transfer and file operations.
- **Local shell tabs** — open a shell in the workspace alongside remote sessions.

## Screenshots

Current screenshots in [`docs/screenshots/`](docs/screenshots/) are from earlier UI iterations and may not fully reflect the latest workspace layout:

- [`docs/screenshots/v0.1-shell.png`](docs/screenshots/v0.1-shell.png)
- [`docs/screenshots/v0.1-config-panel.png`](docs/screenshots/v0.1-config-panel.png)
- [`docs/screenshots/v0.1-ssh-session.png`](docs/screenshots/v0.1-ssh-session.png)

## Requirements

### Runtime / platform

- Linux (primary target). Tauri bundling is configured for desktop targets; packaged outputs depend on your build environment.
- System OpenSSH client tools used by features at runtime:
  - `ssh` — terminal sessions
  - `sftp` — SFTP commander
  - `ssh-agent`, `ssh-add`, `ssh-keygen` — key manager / agent workflows
- Optional tools for import workflows:
  - `puttygen` — convert PuTTY `.ppk` keys during import
  - `keepassxc-cli` — import from `.kdbx` files

### Build prerequisites

- Node.js 20+ recommended (CI uses Node 20) and npm
- Rust stable toolchain and Cargo
- Tauri 2 Linux system dependencies (WebKitGTK/GTK and related libraries). On Debian/Ubuntu, packages similar to:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for distro-specific details.

## Getting started

```bash
# Install frontend dependencies
npm install

# Run in development mode (Vite + Tauri)
npm run tauri dev

# Type-check and build the frontend
npm run build

# Build desktop bundles via Tauri
npm run tauri build
```

Build artifacts are produced under `src-tauri/target/release/bundle/` (exact bundle formats depend on platform/tooling).

## Usage overview

### Connections

- Use the **Connections** rail to organize hosts in folders, select/edit connection details, and open sessions.
- Double-click a connection or use menu/context actions to connect.
- The configuration panel (toggleable) edits connection properties such as host, port, user, key path, proxy/X11/agent-forwarding options, and login/command metadata.

### Workspace

- Tabs can host **terminal** sessions, **local shells**, and **SFTP commander** views.
- Drag tabs to rearrange or move them between panes; split the active pane from the View menu or pane controls.
- Keyboard shortcuts include:
  - `Ctrl+T` — new local shell
  - `Ctrl+W` — close current tab
  - `Ctrl+Shift+C` — copy terminal selection
  - `Ctrl+V` — paste into terminal
  - `Ctrl+,` — toggle Settings page

### Import / export

- **File → Import connections…** opens the import wizard (scan, preview, map, merge/replace, and optional key conversion).
- **File → Export connections…** writes a WinSSHTerm-format `connections.xml`.
- Passwords are not written to connection XML exports; they are stored in the encrypted vault when available.

### Vault

- On first use you can create a master password; existing vaults prompt for unlock.
- Vault state is surfaced in the UI/status bar; lock/reset actions are available from menus/dialog flows.
- Password edits for connections are persisted to the vault while unlocked (and handled according to current app state when locked).

### SSH key manager

- **Tools → SSH Key Manager** shows agent status and loaded identities.
- The app can adopt an existing agent socket or manage a dedicated agent for the application.
- Add/remove keys as needed; passphrase-protected keys are supported via askpass prompts.

### SFTP commander

- Open **Copy Files / SFTP Commander** for a configured connection.
- Provides local and remote panes with navigation, transfer, rename, mkdir/delete, permissions/property actions (subject to backend/server capabilities), and shortcuts aligned with the commander UI.

## Configuration and data

NuxSSHTerm stores user data under `~/.config/nuxsshterm/` (platform config directory):

| Path | Purpose |
| --- | --- |
| `connections.xml` | Session tree in WinSSHTerm XML format (passwords stripped) |
| `settings.json` | Non-secret app/UI settings (UI preferences, agent record, per-session flags, etc.) |
| `vault.bin` | Encrypted vault data (secrets such as stored passwords / import mappings) |
| `log` | Backend log output (best effort) |
| `imported/` (when used) | Converted/imported key material created by import flows |
| `*-askpass.sh` / helper files (runtime-created) | Temporary/helper scripts used for askpass-driven auth flows |

> **Note:** Exact auxiliary filenames may vary by feature/runtime; do not treat them as a stable public API.

## Project structure

- [`src/`](src/) — React + TypeScript frontend (UI, hooks, API bindings)
- [`src-tauri/src/`](src-tauri/src/) — Rust backend (Tauri commands, PTY/SSH/SFTP/vault/import/agent logic)
- [`docs/`](docs/) — project documentation
- [`plans/`](plans/) — design and modernization planning notes

## Documentation

- [Documentation index](docs/README.md)
- [Architecture](docs/ARCHITECTURE.md)
- [UI notes/spec](docs/UI-SPEC.md)

## Development notes

- Frontend dev server is configured for Tauri (`vite.config.ts`, fixed port behavior).
- CI (`.github/workflows/ci.yml`) runs frontend build and backend `cargo check`, `clippy`, and `test` on Linux.

## License

This project is licensed under the GNU General Public License v3.0. See [`LICENSE.txt`](LICENSE.txt).
