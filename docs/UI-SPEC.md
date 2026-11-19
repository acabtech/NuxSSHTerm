# NuxSSHTerm — UI Notes

This document describes the current UI at a high level and retains selected historical notes from the original WinSSHTerm-parity effort for context.

> **Status:** the application UI has evolved beyond the initial v0.1 parity mock/spec. For the current product overview, see [`../README.md`](../README.md). For implementation details, see [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Current UI overview (source-aligned)

### Application shell

- **Menu bar:** `File`, `Edit`, `View`, `Sessions`, `Tools`, `Help` menus plus a workspace/settings page badge.
- **Workspace page:** primary working area with connections rail, configurable side panel, and tileable tabbed work area.
- **Settings page:** dedicated page for appearance, terminal, connections/security, transfer, and shortcuts/about sections (with live-persisted settings where supported).
- **Status bar and toasts:** runtime status/feedback surfaces for vault state, actions, and notifications.

### Connections rail and configuration

- A left-side **Connections** rail shows the session tree (folders/connections), search/organization affordances, and context actions.
- The rail supports modern layout behavior (configurable width/collapse behavior in settings; hover/pin interactions implemented in the app shell).
- A **Configuration** panel is available under/alongside the tree for editing the selected node’s properties (connection/container fields, auth/proxy/forwarding options, login/command metadata, etc.).

### Workspace, tabs, and panes

- Tabs can host:
  - **Terminal** sessions (remote SSH or local shell)
  - **SFTP commander** views
- Tabs are draggable/reorderable and can be moved between panes.
- The work area supports **split panes** (horizontal/vertical tiling) and pane resizing.
- The app keeps tab content mounted so session state survives view/layout changes (portal-based rendering model).

### Menus and key actions (current intent)

Common actions exposed through menus/UI include:

- **File:** new local shell/connection/folder, import/export connections, open config folder, lock vault, settings, quit
- **Edit:** copy/paste terminal content (and placeholders for future features where marked)
- **View:** toggle configuration panel, split panes, switch workspace/settings
- **Sessions:** connect selected, duplicate, disconnect, open SFTP commander
- **Tools:** SSH key manager, master password/vault actions
- **Help:** shortcuts, documentation/about dialogs

### Keyboard shortcuts (documented in UI)

- `Ctrl+T` — new local shell
- `Ctrl+W` — close current tab
- `Ctrl+Shift+C` — copy terminal selection
- `Ctrl+V` — paste into terminal
- `Ctrl+,` — toggle Settings page
- Drag/drop gestures — move tabs between panes / split layouts (per workspace UI behavior)

## Feature-specific UI notes

### Import/export

- **Import wizard** provides scan/preview/mapping and merge-or-replace flows for supported source formats.
- **Export** writes WinSSHTerm-format session XML (secrets excluded from XML).

### Vault UX

- First-run/setup and unlock flows are presented via modal dialogs.
- Vault state is surfaced in the UI (e.g. status indicators/badges/messages).
- Lock/reset actions are available through menu/dialog workflows.

### SSH key manager

- Modal/panel workflow for agent status, listing identities, adding/removing keys, and handling passphrase prompts when required.
- Integrates with adopted/external agents or an app-managed dedicated agent (backend-driven behavior).

### SFTP commander

- Dual-pane local/remote interface with path navigation and file operations.
- Commander tabs share the workspace tab system with terminal tabs.
- Progress/status feedback is surfaced in the commander UI (capabilities depend on the current backend driver).

### Settings

- Settings are grouped into sections such as Appearance, Terminal, Connections & Security, File Transfer, and Shortcuts/About.
- Some settings persist immediately/live-apply to new sessions or UI chrome (see implementation for exact scope per setting).

## Design direction

The modernized UI follows the project design/planning artifacts:

- [`../plans/DESIGN.md`](../plans/DESIGN.md) — visual tokens and component direction
- [`../plans/modernization-plan.md`](../plans/modernization-plan.md) — IA and modernization scope

These documents describe intended direction; the source code remains the authoritative reference for current behavior.

## Historical notes: WinSSHTerm parity target (v0.1 era)

The following notes are retained as historical context from early parity work and may not match the current implementation.

### Original parity goals

- Approximate WinSSHTerm-style session management, tabbed workspace, and familiar workflows (connect, edit, copy files/SFTP, key/vault-related utilities).
- Reuse WinSSHTerm session XML concepts to ease migration/import/export.

### Early layout sketch (historical)

The original parity spec targeted a fixed/chrome-heavy layout similar to WinSSHTerm:

- top menu/quick strip
- left connections/configuration panels
- right tabbed document area (terminal/commander)
- Windows-like visual styling (later modernized)

### Migration validation context (historical)

Early testing used real-world session-tree samples as acceptance fixtures (folder hierarchies and mixed host sets). This remains useful background for import/export compatibility testing, but it is not a complete specification of current UI behavior.

## Documentation maintenance

When UI behavior changes, update this document together with:

- [`../README.md`](../README.md) — user-facing capabilities and shortcuts
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — backend/frontend responsibilities and data flows
- Screenshots in [`screenshots/`](screenshots/) when they no longer represent the UI
