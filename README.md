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

### Command-line interface (CLI)

The `nuxsshterm` binary doubles as a headless CLI for scriptable/LLM-driven
operations. Without a recognized subcommand the GUI starts, as before.

```bash
nuxsshterm list                                  # session tree flattened to stable tree paths
nuxsshterm list --json                           # machine-readable output
nuxsshterm status                                # store presence, session counts, vault state
nuxsshterm paths                                 # configuration/data file paths
nuxsshterm import connections.xml                # preview a sessions file (no changes)
nuxsshterm import connections.xml --yes          # commit: replaces the stored session tree
nuxsshterm export out.xml                        # export WinSSHTerm XML (passwords stripped)
nuxsshterm export out.xml --force                # overwrite an existing target
nuxsshterm apikeys create automation             # mint a bearer key (plaintext shown once)
nuxsshterm apikeys list                          # key metadata only, never secrets
nuxsshterm apikeys revoke <id>                   # revoke a key and its wrapped vault copy
nuxsshterm vault status                          # opt-in auto-unlock configuration
nuxsshterm vault unlock                          # verify the configured key in memory
nuxsshterm omarchy status                        # Omarchy detection / published count
nuxsshterm omarchy sync                          # publish favorite sessions
nuxsshterm omarchy disable                       # remove managed menu rows
nuxsshterm --session "grp/Prod"                  # launch the GUI opening one session
```

Conventions (applies as the surface grows):

- Data goes to **stdout**; diagnostics and warnings go to **stderr**.
- `--json` yields stable machine-readable output; exit codes are `0` success,
  `1` runtime error, `2` usage error.
- Non-interactive: no prompts — missing prerequisites fail fast with a clear
  error.
- Secrets are never printed: `vault unlock` only proves that the configured
  auto-unlock key can unwrap the vault key in memory; the vault stays locked
  headless. Exports always strip passwords, and imports never store passwords
  (use the GUI import wizard for vault-backed imports).
- Sessions are addressed by their `/`-joined tree path (same key scheme the
  vault and settings use).

Terminal sessions, SFTP, and the agent manager require the GUI.

### Vault

- On first use you can create a master password; existing vaults prompt for unlock.
- Vault state is surfaced in the UI/status bar; lock/reset actions are available from menus/dialog flows.
- Password edits for connections are persisted to the vault while unlocked (and handled according to current app state when locked).

#### Opt-in automatic unlock with an API key

**Settings → Connections & Security → Automatic vault unlock** is disabled by
default. To enable it, create an API key (`apikeys create <name>`), supply that
key once in the card together with the existing master password, and choose a
local-only source for subsequent unlocks:

- an **environment variable** (default `NUXSSH_API_KEY`, or a custom name), or
- a **protected local file** (paths may start with `~/`).

Enabling wraps the vault's AES key with a key-encryption-key derived from the
API key and stores only that wrapped blob in `auto-unlock.json`. The master
password is never persisted. Revoking the key — from the UI or the CLI —
deletes the wrapped copy and switches the setting off. **Disable and forget
wrapped key** does the same without revoking the key itself. Vault reset also
clears the wrapper. The normal master-password path and inactivity auto-lock
continue to work unchanged; locking drops the in-memory key, and the next
startup may use the key again.

Threat model and limitations:

- **Protects against:** offline reading of `vault.bin` or `settings.json`
  alone; an attacker needs both the wrapped key file and the API key plaintext
  from the chosen local source.
- **Does not protect against:** a compromised user account or machine, another
  process reading the environment/file, backups that include both the wrapper
  and the key, or coercion of the user. Anyone who obtains the API key can read
  every stored session password.
- The API key is a **bearer credential**, not a scoped permission: it is only
  for vault decryption/unlock and CLI identification. Keep the key file
  `0600`, avoid putting it in shell history or shared dotfiles, and disable the
  feature on machines you do not control.
- `vault status`/`vault unlock` never print the key or decrypted passwords;
  headless `vault unlock` validates credentials in memory only.

### Omarchy menu integration

**Settings → Connections & Security → Omarchy menu integration** is opt-in and
a no-op when the Omarchy config is not detected. When enabled, starred
connections are published under Omarchy's existing **Remote Hosts** submenu as
`remote-hosts.nuxsshterm.*` rows in
`~/.config/omarchy/extensions/omarchy-menu.jsonc`.

- The app owns only the lines between its `BEGIN/END NuxSSHTerm managed
  favorites` markers; all other JSONC rows and comments are preserved.
- Selecting a row runs `nuxsshterm --session <stable path>`; the backend queues
  the path and the GUI opens that session when it starts.
- Enabling publishes immediately; renaming, unfavoriting, or deleting a
  session rewrites the block on the next save, and disabling removes it.
- The desktop entry must be installed so `nuxsshterm` resolves on `PATH`.
  No Omarchy QML patch is required — this writes JSONC rows, not a new custom
  provider. Inspect the file before enabling if you hand-maintain that menu.

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
| `auto-unlock.json` (opt-in) | API-key ID plus the AES-GCM-wrapped vault key; never contains the master password or key plaintext |
| `pending-session` (transient) | Stable session path queued by `nuxsshterm --session <path>` until the GUI consumes it |
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
