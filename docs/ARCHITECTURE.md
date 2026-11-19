# NuxSSHTerm — Architecture

This document describes the current architecture and implementation shape of NuxSSHTerm as reflected in the repository (`package.json`/`Cargo.toml` version `0.5.2`).

## Overview

NuxSSHTerm is a desktop SSH workspace for Linux built with:

- **Frontend:** React + TypeScript + Vite
- **Desktop shell:** Tauri 2
- **Backend:** Rust (`src-tauri/src`)
- **Terminal rendering:** xterm.js
- **Session transport (current):** system OpenSSH tools (`ssh`, `sftp`) executed from the backend (PTY/child processes)

The application aims for practical WinSSHTerm workflow parity on Linux while using native system tooling instead of Wine/emulation.

## High-level design

```text
+----------------------------- NuxSSHTerm (Tauri app) -----------------------------+
|  Frontend (React/TS)                                                            |
|  - Workspace UI (menus, tabs/panes, session tree, config, settings)              |
|  - Terminal view (xterm.js)                                                     |
|  - SFTP commander UI                                                            |
|  - Tauri invoke/event API bindings (src/api.ts)                                 |
+---------------------------------------+-----------------------------------------+
                                        | invoke commands / listen events
+---------------------------------------v-----------------------------------------+
|  Backend (Rust, Tauri commands)                                                 |
|  - Session tree load/save/import/export (WinSSHTerm XML)                        |
|  - PTY terminal sessions (system ssh / local shell)                             |
|  - SFTP commander driver (persistent sftp child + local fs ops)                 |
|  - Vault (encrypt/decrypt, password storage)                                    |
|  - SSH agent/key manager integration                                            |
|  - Import pipelines (WinSSHTerm/PuTTY/KiTTY/KeePass helpers)                    |
|  - App settings persistence                                                      |
+---------------------------------------+-----------------------------------------+
                                        | spawns / reads-writes
+---------------------------------------v-----------------------------------------+
|  Local system resources                                                         |
|  - OpenSSH binaries (ssh, sftp, ssh-agent, ssh-add, ssh-keygen)                 |
|  - Optional tools (puttygen, keepassxc-cli)                                     |
|  - User config dir (~/.config/nuxsshterm/*)                                     |
+----------------------------------------------------------------------------------+
```

## Repository layout

| Path | Responsibility |
| --- | --- |
| [`src/`](../src/) | Frontend application: UI components, hooks, layout/state helpers, Tauri API wrappers |
| [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs) | Tauri app bootstrap and command registration |
| [`src-tauri/src/`](../src-tauri/src/) | Backend modules (PTY, SSH/SFTP, vault, store/model/xml, import/agent, etc.) |
| [`docs/`](./) | Documentation |
| [`plans/`](../plans/) | Design and modernization planning artifacts |

## Frontend architecture

### Entry points and shell

- [`src/main.tsx`](../src/main.tsx) boots the React app.
- [`src/App.tsx`](../src/App.tsx) is the primary application orchestrator:
  - owns top-level UI state wiring (workspace/settings page, modals, menus, notifications)
  - composes session tree, tabs, layout, settings, vault, and command actions
  - coordinates tab/pane behavior and menu-driven workflows

### UI domains

- **Session tree & configuration:** [`src/components/SessionTree.tsx`](../src/components/SessionTree.tsx), [`src/components/ConfigPanel.tsx`](../src/components/ConfigPanel.tsx), [`src/hooks/useSessionTree.ts`](../src/hooks/useSessionTree.ts)
- **Workspace/tabs/panes:** [`src/components/TabStrip.tsx`](../src/components/TabStrip.tsx), [`src/components/WorkArea.tsx`](../src/components/WorkArea.tsx), [`src/components/TabContent.tsx`](../src/components/TabContent.tsx), [`src/hooks/useTabs.ts`](../src/hooks/useTabs.ts), [`src/hooks/useLayout.ts`](../src/hooks/useLayout.ts)
- **Terminal:** [`src/components/TerminalView.tsx`](../src/components/TerminalView.tsx) (xterm.js integration + PTY I/O/events)
- **SFTP commander:** [`src/components/CommanderView.tsx`](../src/components/CommanderView.tsx)
- **Settings:** [`src/components/SettingsPage.tsx`](../src/components/SettingsPage.tsx), [`src/hooks/useSettings.ts`](../src/hooks/useSettings.ts)
- **Vault / import / keys:** [`src/components/VaultModal.tsx`](../src/components/VaultModal.tsx), [`src/components/ImportWizard.tsx`](../src/components/ImportWizard.tsx), [`src/components/KeyManagerModal.tsx`](../src/components/KeyManagerModal.tsx), [`src/hooks/useVault.ts`](../src/hooks/useVault.ts)
- **Shared UI/utilities:** menus, status, toasts, icons, tree/color/menu helpers in [`src/components/`](../src/components/) and [`src/lib/`](../src/lib/)

### Frontend-backend boundary

- [`src/api.ts`](../src/api.ts) defines typed wrappers around Tauri `invoke` calls and event listeners.
- [`src/types.ts`](../src/types.ts) contains shared frontend types (session nodes, launch specs, UI settings, layout/tab types, etc.) aligned with backend serialization.

## Backend architecture

### Tauri command surface

[`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs) registers the main command groups:

- **Session/config storage:** `load_tree`, `save_tree`, `get_ui_settings`, `set_ui_settings`, `config_dir`, import/export commands
- **PTY/terminal:** `pty_open`, `pty_write`, `pty_resize`, `pty_close`, `pty_alive`
- **Vault:** status/init/unlock/lock/reset/get/put/remove password and PPK-map helpers
- **Import helpers:** session-file scan/import, `puttygen` availability/conversion, `keepassxc-cli` availability
- **SFTP + local filesystem:** open/close/list/op for remote SFTP sessions and local file operations
- **SSH agent/key manager:** status/start/stop/add/remove/remove-all

Managed backend state includes PTY sessions, vault state, SFTP sessions, and agent-related state.

### Core backend modules

| Module | Purpose |
| --- | --- |
| [`model.rs`](../src-tauri/src/model.rs) | Session tree/node model (WinSSHTerm-compatible fields) and helpers (flattening, password stripping) |
| [`xml.rs`](../src-tauri/src/xml.rs) | Parse/write WinSSHTerm XML structures |
| [`store.rs`](../src-tauri/src/store.rs) | Persistence under config dir (`connections.xml`, `settings.json`), UI settings, agent record, forward-agent mapping, safe export behavior |
| [`vault.rs`](../src-tauri/src/vault.rs) | Encrypted vault format and operations (Argon2id KDF + AES-256-GCM) |
| [`ssh.rs`](../src-tauri/src/ssh.rs) | Launch specification and argv construction for `ssh`/`sftp` |
| [`pty.rs`](../src-tauri/src/pty.rs) | PTY lifecycle, process spawning, I/O streaming events, resize/close, auth helper integration |
| [`sftp.rs`](../src-tauri/src/sftp.rs) | Persistent `sftp` child driver, command/response framing, remote operations, askpass integration |
| [`local_fs.rs`](../src-tauri/src/local_fs.rs) | Local filesystem operations for commander views |
| [`agent.rs`](../src-tauri/src/agent.rs) | SSH agent discovery/adoption/spawn, key operations, socket export support |
| [`importcmd.rs`](../src-tauri/src/importcmd.rs), [`putty.rs`](../src-tauri/src/putty.rs), [`kdbx.rs`](../src-tauri/src/kdbx.rs), [`wsst_crypto.rs`](../src-tauri/src/wsst_crypto.rs) | Import detection/parsing and conversion support for WinSSHTerm/PuTTY/KiTTY/KeePass-related inputs |
| [`log.rs`](../src-tauri/src/log.rs) | Lightweight backend logging |
| [`cli.rs`](../src-tauri/src/cli.rs) | Headless CLI (`list`/`status`/`paths`/`import`/`export`): argument dispatch in `run()` before the GUI starts, `--json` output, stable exit codes (0/1/2), stdout=data / stderr=diagnostics; reuses store/model/xml/importcmd without Tauri state |

## Data model and persistence

### Session tree (`connections.xml`)

- Stored in WinSSHTerm-compatible XML format to support round-tripping/import/export workflows.
- **Passwords are stripped before save/export** (`model::strip_passwords` / store export paths) to avoid persisting plaintext secrets in XML.
- UI-local/non-XML fields are handled separately (e.g. per-session forward-agent flags are stored in `settings.json`).

### Application settings (`settings.json`)

`settings.json` stores non-secret application state, including:

- Dedicated SSH agent record (socket/PID metadata used by the app-managed agent flow)
- Per-session forward-agent flags (keyed by session path)
- UI settings (`UiSettings`) such as density, font scale, accent color, sidebar behavior, terminal preferences, transfer preferences, auth preferences, and defaults for new connections

### Encrypted vault (`vault.bin`)

- Purpose: store sensitive values (notably connection passwords and import-related secret mappings such as PPK conversion records) encrypted at rest.
- Crypto primitives (per backend implementation): **Argon2id** for key derivation and **AES-256-GCM** for authenticated encryption.
- Vault lifecycle is exposed to the frontend via Tauri commands (init/unlock/lock/reset and CRUD-style secret accessors).

### Other config-dir artifacts

Depending on features used, the config directory may also contain:

- backend log file (`log`)
- imported/converted key outputs (e.g. under an `imported/` area)
- askpass helper scripts/files used transiently for password/passphrase flows

## Runtime behavior (key flows)

### Terminal sessions

1. Frontend requests `pty_open` with a launch spec (and optional password context).
2. Backend resolves command/args/environment (system `ssh` for remote targets, or local shell path as applicable).
3. A PTY is created and the child process is spawned.
4. Output is streamed to the frontend via Tauri events; frontend writes to xterm.js.
5. Input/resize/close operations are forwarded to the backend.

This design inherits OpenSSH behavior/configuration (`~/.ssh/config`, known hosts, agent integration, proxy options mapped into SSH args, etc.).

### SFTP commander

1. Frontend opens an SFTP session (`sftp_open`) for a target.
2. Backend maintains a persistent `sftp` child process and executes framed commands sequentially.
3. Directory listings and file operations are parsed/returned to the UI; local pane operations use backend local-fs commands.
4. Transfers/operations are orchestrated at the commander level (current implementation favors correctness and compatibility over in-process transfer engine features).

### SSH agent / key management

- Agent status is derived from environment and/or app-managed agent records.
- The app can spawn and track a dedicated agent (persisting its record in settings) or adopt an existing agent.
- Key operations (`ssh-add`/listing/removal/fingerprint support) are surfaced through Tauri commands.
- Agent socket export to spawned session processes enables key-based auth and optional forwarding.

### Import/export

- **Import:** detects/parses supported formats (WinSSHTerm XML/settings, PuTTY `.reg`, KiTTY `.txt`, KeePass `.kdbx` where tooling is available), produces a preview/mapping model, and merges/applies results with warnings where needed.
- **PPK conversion:** uses `puttygen` when available; converted outputs can be recorded for reuse.
- **Export:** writes WinSSHTerm-format XML with secrets removed from the XML payload.

## Security considerations

- **No plaintext passwords in session XML:** passwords are stripped from saved/exported XML and stored via the encrypted vault mechanism.
- **Local secret handling:** vault data and key-related artifacts live under the user's config directory; runtime helpers (e.g. askpass scripts) are created only as needed by flows.
- **System SSH delegation:** authentication/host verification behavior largely follows OpenSSH tooling and user/system SSH configuration.
- **Logging:** backend logs are intended for diagnostics; sensitive material should not be emitted (review module behavior when changing auth flows).

## Testing and CI

- **Frontend:** TypeScript build (`npm run build`, which runs `tsc && vite build`).
- **Backend:** Rust checks/tests/lints are used in CI (see [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)).
- The repository includes Rust unit/integration-style tests in backend modules (including ignored/smoke tests in some areas that require local environment support).

## Known limitations / evolution areas

- Terminal and SFTP operations currently rely on external OpenSSH binaries rather than an in-process SSH/SFTP library implementation.
- Transfer progress/reporting and advanced transfer controls are constrained by the current scripted `sftp` driver approach.
- Some UI/documentation artifacts in the repo (screenshots and historical spec notes) may lag behind the current `v0.5.x` interface; prefer source-backed docs when in doubt.

## Related documents

- [`README.md`](../README.md) — user-facing overview and usage
- [`docs/UI-SPEC.md`](UI-SPEC.md) — UI behavior notes (current + historical context)
- [`plans/DESIGN.md`](../plans/DESIGN.md) and [`plans/modernization-plan.md`](../plans/modernization-plan.md) — modernization design context
