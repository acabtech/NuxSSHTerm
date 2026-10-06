# UI Spec — WinSSHTerm parity target (from Sam's screenshot, 2026-10-04)

Reference: `winsshterm-screen.PNG` — WinSSHTerm 2.43.x on Windows, window maximized at ~1920×1040.
Layout engine in the original: **DockPanel Suite** (dockable panels). v0.1 = fixed layout; v0.2 = draggable/dockable.

## 1. Window chrome

```
┌───────────────────────────────────────────────────────────────────────────────────────┐
│ WinSSHTerm                                                            ─  □  ✕        │  ← native OS title bar (Tauri)
├───────────────────────────────────────────────────────────────────────────────────────┤
│ File  View  Navigate  Tools  Help  Cons⌨   ·····  Scripts⟳⊠ │ Paste │ Visible │ Con │ None │  ← menu + quick strip
├──────────────────────────────────┬────────────────────────────────────────────────────┤
│ Connections                   ✕ │ More-Services │ LLAMA Backend │ Jakiro Four Local │ **OpenCode Sam** │
│ [←][→][▾][▴][⟳][🔍]              │ ┌────────────────────────────────────────────────┐ │
│  📁 Cod IoT Admin                │ │                                                │ │
│  📁 Cod WMS Prod                 │ │   sam@opencode-sam:~$ █                        │ │
│  📁 Cod Postgres Prod            │ │                                                │ │
│  📁 Beleaf                       │ │                                                │ │
│  📁 Daya Tani                    │ │                                                │ │
│  📁 Personal Machines            │ │                                                │ │
│  ▾ 📁 Proxmox Hosts              │ │                                                │ │
│     🖥 Elitedesk One Host Local  │ │                                                │ │
│     🖥 Elitedesk Two Host Local  │ │                                                │ │
│     🖥 ThinkCentre Three Host…   │ │                                                │ │
│     🖥 Jakiro Four Local         │ │                                                │ │
│     🖥 Ryzen Five Host Local     │ │                                                │ │
│     🖥 opnSense Local            │ │                                                │ │
│  ▾ 📁 Proxmox VMs                │ │                                                │ │
│     🖥 Kiots-Knots               │ │                                                │ │
│     🖥 Home-Services             │ │                                                │ │
│     🖥 More-Services             │ │                                                │ │
│     🖥 Teleport Backend          │ │                                                │ │
│     🖥 TC3-Odoo-IoT              │ │                                                │ │
│     🖥 Hermes Agent Sam          │ │                                                │ │
│     🖥 Global Postgres           │ │                                                │ │
│     🖥 LLAMA Backend             │ │                                                │ │
│     🖥 Hermes Agent John         │ │                                                │ │
│     🖥 **OpenCode Sam**  ◄ sel   │ │                                                │ │
│     🖥 Zabbix Home               │ │                                                │ │
│     … (16 items)                 │ │                                                │ │
│  ▾ 📁 Cloud VMs                  │ └────────────────────────────────────────────────┘ │
│     🖥 AWS CDA Trampoline        │                                                    │
├──────────────────────────────────┤                                                    │
│ Configuration                 ✕  │                                                    │
│ ┌ Connection ─────────────────┐  │                                                    │
│ │ Name          OpenCode Sam  │  │                                                    │
│ │ Host/IP       192.168.100.126│ │                                                    │
│ │ Port          22            │  │                                                    │
│ │ User          sam           │  │                                                    │
│ │ Password      ••••••••      │  │                                                    │
│ │ Private Key   E:\Box Sync\… │  │                                                    │
│ │ Certificate   (empty)       │  │                                                    │
│ │ Login Dir     (empty)       │  │                                                    │
│ │ Login Cmds    (empty)       │  │                                                    │
│ │ Cmd-line Args (empty)       │  │                                                    │
│ │ Env Color     (empty)       │  │                                                    │
│ │ Custom Id     (empty)       │  │                                                    │
│ │ Custom Type   Downloads     │  │                                                    │
│ └─────────────────────────────┘  │                                                    │
└──────────────────────────────────┴────────────────────────────────────────────────────┘
```

## 2. Panels (left column, width ≈ 320 px)

### 2.1 "Connections" (top, ≈ 68% of column height)
- Dock title bar: caption `Connections` left, `✕` close right.
- Toolbar row (icon buttons, flat): **back ←**, **forward →**, **collapse-all ▾**, **expand-all ▴**, **refresh ⟳**, **search 🔍**.
- Tree widget: white background, 1 px dot-grid guides for nesting, folder icon for containers
  (expandable via ▸/▾ triangle), monitor icon for connections.
- Selection: single row, light-blue/gray highlight (`#cce8ff`-ish), no multi-select visible.
- Interactions: double-click connection → open terminal tab; right-click → context menu
  (Connect, Copy Files, Edit, Duplicate, Delete, Export…); drag-drop to re-parent (v0.2).

### 2.2 "Configuration" (bottom, ≈ 32%)
- Dock title bar: caption `Configuration`, `✕` close.
- Header row: dropdown `Connection` (▾) — a **type selector**; switches property set by node type.
- Property grid (WinForms PropertyGrid style): two columns — bold-ish name (left, ~40%), value (right,
  editable). Sets per type:
  - **Connection**: Name, Host/IP, Port, User, Password, Private Key, Certificate, Forward Agent,
    Login Dir, Login Cmds, Cmd-line Args, Env Color, Custom Id, Custom Type
    Login Cmds, Cmd-line Args, Env Color, Custom Id, Custom Type
  - **Container** (implied): Name, Descr
- Edits apply to the selected node immediately (v0.1: edit form on the right; property grid parity v0.2).
- **Forward Agent** (Phase 4): a checkbox row for connections. When checked, sessions launch with
  `-o ForwardAgent=yes` and the sftp commander forwards the agent too. UI-local setting persisted
  in `settings.json` keyed by the session path (never exported to the WinSSHTerm XML).
- **Password field**: never written to `connections.xml` (stripped on save/export). When the vault
  is unlocked, edits are persisted to the encrypted vault (`vault.bin`); when locked, they are held
  in memory only. A note under the property grid reflects the current vault state.

## 3. Document area (tabs)

- Tab strip immediately under the menu bar, full remaining width.
- Tabs are **closable**, reorderable by drag (v0.2), with per-tab context menu (Close, Close Others,
  Close All, Rename, Duplicate).
- Active tab: light background, dark text, raised border. Inactive: flat, gray text.
  From the screenshot: active `OpenCode Sam` is the 4th of `More-Services`, `LLAMA Backend`,
  `Jakiro Four Local`, `OpenCode Sam` — active tab is visually distinct (lighter/white).
- Content: terminal (xterm.js) or commander (SFTP) — same tab strip for both.

## 4. Terminal rendering

- Background: very dark navy ≈ `#0d1b2a` / `#101820` (PuTTY-ish dark). Sampled later precisely.
- Foreground: light gray ≈ `#d0d0d0`; prompt shows `sam@opencode-sam:~$`.
- Cursor: block, blinking.
- Font: monospace (Consolas-like) → use `JetBrains Mono`/`DejaVu Sans Mono` on Linux, 11–12 pt.
- Scrollbar on the right, full-height, classic (non-overlay) look.
- PuTTY default ANSI palette to be matched; Sam's "Env Color" per-session is a WinSSHTerm feature (v0.2).

## 5. Top-right quick strip (from screenshot)

Buttons, left→right: `Scripts` (+ ⟳ reload, ⊠ clear icons), separator, `Paste`, `Visible`, `Con`, `None`.
- `Scripts` = LaunchTools/PowerShell automation menu (v0.3 on Linux: shell scripts).
- `Paste` = paste clipboard into active terminal.
- `Visible` / `Con` / `None` = quick-launch-bar visibility presets.
v0.1: render the strip for fidelity; wire `Paste` only. Others are placeholders.

## 5b. Notifications (toasts)

- Status/error messages appear as a stack of auto-expiring, dismissible toasts in the top-right
  corner (info/success/error variants, colour-coded left border). The status bar keeps the latest
  line. Replaces the earlier ad-hoc `notice` strings.

## 5c. Encrypted vault (v0.2)

- **First run** → "Set master password" wizard modal (password + confirm; min 4 chars). Dismissable
  via **Skip** (passwords stay memory-only until a vault is created).
- **Every start** → "Unlock vault" modal when a vault exists. **Forgot password?** leads to a
  **Reset vault** confirmation (wipes stored passwords; connections are kept).
- **File → Master password…** opens the wizard or unlock dialog (or shows vault status when
  already unlocked); **File → Lock vault** drops the in-memory key and clears passwords from the UI.
- **Status bar** shows `vault: unset | locked | unlocked` (green when unlocked, amber when locked).
- While unlocked, editing a session's Password in the Configuration panel persists it to the
  encrypted vault; the note under the property grid reflects the current state.

## 5d. Import wizard (v0.2.1)

- **File → Import…** opens the import wizard (replaces the old plain-path import modal):
  a path box + **Scan** → the backend sniffs the file (PuTTY `.reg` — UTF-16LE/UTF-8 with
  `%XX` names; KiTTY `.txt`; WinSSHTerm `connections.xml`/`.settings`; KeePass `.kdbx` via
  `keepassxc-cli`) and returns a preview + warnings.
- **Preview table** (tree-flattened, indented): checkbox per row, folder checkboxes cascade to
  their sessions; per-session inline edits for Host / User / Port / Private key.
- **Warnings banner** (amber): absolute Windows `C:\…` key paths, unsupported `ProxyMethod`,
  skipped kdbx entries, missing `puttygen`/`keepassxc-cli`.
- **Vault note** (blue): how many source passwords were found and whether the (unlocked/ locked)
  vault will store them — passwords are never shown in the preview.
- **Import**: `.ppk` keys are converted on the spot via `puttygen -O private-openssh` into
  `<config>/imported/` (600; originals untouched); sessions merge into the tree (default) or
  replace it, then save; passwords + `.ppk` mappings go into the vault when unlocked.

## 5e. SFTP commander (v0.3.0)

WinSCP "commander view" parity, opened per session via **right-click → Copy
Files** (or Navigate → Copy Files). One commander tab per open; it shares the
tab strip with terminals (`Terminal (SFTP)` suffix in the tab title).

- **Dual panes**: `Local` (left) and `Remote — user@host` (right), each with a
  title bar, an editable **path bar** (Enter to navigate, ⬆ up button),
  column headers (Name / Size / Modified), and a scrollable listing.
  Focused pane gets a blue border; click a row to select (single-select,
  WinSCP-style multi-select is deferred).
- **Listing**: folders first (📁), then files (📄) and symlinks (🔗), sorted by
  name. The `..` row navigates up. Columns: perms (tooltip), size (humanised;
  blank for folders), server-side mtime/perms string for remote, local
  `YYYY-MM-DD HH:MM` for local.
- **Toolbars** per pane: 📁+ new folder, 🗑 delete, ℹ properties.
- **Keybindings** (focused pane): **F5** copy to the other pane, **F6** move
  (copy then delete source — WinSCP semantics), **F7** mkdir, **F8** delete
  (folders recursive, with confirm), **F9** properties, **Ctrl+U** swap active
  pane, **Ctrl+R** refresh, **Ctrl+T** open a terminal tab to the same host,
  **Enter** open folder, **Backspace** parent dir.
- **Properties modal (F9)**: type, path, perms, size, owner:group, modified;
  remote files get a **chmod…** action (numeric mode).
- **Transfer progress strip**: bottom bar with an activity dot while a
  transfer runs + the current operation text (“Downloading X…”); file-level
  only in v0.1 (byte-level progress arrives with the russh engine, Phase 6).
- **Deletions** of remote folders run a depth-first walk over the persistent
  sftp child (`rm` files / `rmdir` dirs — OpenSSH sftp has no `rm -r`).

## 5f. SSH key manager (v0.4.0)

Pageant-equivalent, opened via **Tools → SSH key manager…** (or the Tools menu hint “Pageant”).
One modal; nothing else changes in the main chrome.

- **Status line**: green dot — an agent is reachable; grey — none. Shows whether the socket
  is the **dedicated agent** (spawned by NuxSSHTerm) or an **external agent** (`SSH_AUTH_SOCK`
  inherited from the environment), plus the socket path.
- **Start agent** (only when nothing is running): spawns a dedicated `ssh-agent -a
  ~/.config/nuxsshterm/agent.sock` in daemon mode; keys survive app restarts (the record lives
  in `settings.json`). **Stop agent** (only for the dedicated one) kills it via `ssh-agent -k`.
- **Identity table**: bits, fingerprint, comment, type (`ssh-add -l`). Empty state: “The agent
  has no identities.” Rows list a **Remove** button when the key was added by NuxSSHTerm this
  session (needs the key path); other rows show “—” with a tooltip.
- **Add key…**: native file picker → `ssh-add <key>`. A passphrase-protected key shows an inline
  passphrase prompt inside the modal (askpass helper, like the sftp one) and retries until added.
  Global **Remove all** clears every identity (`ssh-add -D`). **Refresh** re-queries.
- **Forward Agent** checkbox in the Configuration panel (see 2.2) controls agent forwarding
  per session, independent of the key manager itself.

## 6. Theme

Windows-native look: `#f0f0f0` chrome, `#ffffff` panels, `#000000` text, `#e8eef5` tab-strip tint,
blue accent for selection. Reproduce with CSS in v0.1 (Windows-like), then offer a Hyprland-native
dark theme in v0.2 so it sits well on Omarchy Quattro.

## 7. Sam's real tree → import validation dataset

The screenshot doubles as our **acceptance test fixture**: 4 top-level collapsed containers
(Cod IoT Admin, Cod WMS Prod, Cod Postgres Prod, Beleaf, Daya Tani, Personal Machines),
`Proxmox Hosts` (6 hosts), `Proxmox VMs` (16 VMs incl. duplicates like Home-Services/More-Services
that also appear as tabs), `opnSense Local`, `Debian WSL`, `Grandstream`, `Cloud VMs` (1).
After import, the rendered tree must be visually identical to this screenshot.