# NuxSSHTerm — Architecture (v0.4.0)

A Linux-native reimplementation of WinSSHTerm — **no Wine, no Windows emulation** — targeting Sam's
Omarchy Quattro system, to replace his Windows SSH workflow completely.

WinSSHTerm on Windows is: PuTTY (terminal) + WinSCP (SFTP) + Pageant (keys) + KeePass (vault) +
XML settings, glued by a tabbed manager. This project maps every piece to a native Linux counterpart.

## Stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri 2 (Rust)** | ~15 MB native binary, usercode in Rust; user-selected |
| UI | **React 18 + TypeScript + Vite** | Fast UI iteration, CSS can faithfully clone the Windows chrome |
| Terminal | **xterm.js** (frontend) + **portable-pty** (Rust) | Renders a real PTY; battle-tested combo (Tabby et al.) |
| SSH session | **system `ssh` (openssh-client)** via PTY | v0.1 robustness: inherits `~/.ssh/config`, agent, keys, X11, ProxyJump — zero SSH-library risk |
| SFTP | **system `sftp`** batch-mode driver | Same inheritance benefits; structured `ls -la` parsing |
| Keys / Pageant | **ssh-agent** (native) + UI wrapper around `ssh-add` | Pageant-equivalent: add/list/remove/fingerprints/forwarding |
| .ppk conversion | **puttygen** (putty-tools) | Convert PuTTY keys → OpenSSH at import time (original untouched) |
| Master password / vault | **Argon2id + AES-256-GCM** (Rust `argon2`, `aes-gcm`) in-process | No KeePass dependency; optional .kdbx import via `keepassxc-cli` later |

Later upgrade path (v0.2+): move SSH/SFTP in-process with `russh` + `russh-sftp` once the UX and
feature set are proven; system binaries remain the fallback.

## Data model (mirrors WinSSHTerm exactly)

WinSSHTerm session tree XML (verified from `Migrate2WinSSHTerm.py`, v0.23 — real schema):

```xml
<WinSSHTerm Version='1'>
  <Node Name='Base64(path)' Type='Container' Expanded='True'>
    <Node Name="display name" Type="Connection"
          Descr="" Username="sam" Password="" PrivateKey="C:\keys\id.ppk"
          Hostname="192.168.100.201" Port="22" Certificate=""
          LaunchToolInt="" sX11="don't forward" cfProt="sftp"
          pSshProxy="enabled|disabled" pType="SOCKS4|SOCKS5|HTTP|Local"
          pHost="" pPort="" pUser="" pTelnetCmd="" />
  </Node>
</WinSSHTerm>
```

- Containers: base64(UTF-8 path segment); Connections: attributes above.
- `pSshProxy=disabled` + empty proxy attrs = no proxy.
- PuTTY registry values (source for import): `HostName`, `PortNumber`, `UserName`, `PublicKeyFile`,
  `DetachedCertificate`, `ProxyMethod` (1=SOCKS4, 2=SOCKS5, 3=HTTP, 5=Local), `ProxyHost`,
  `ProxyPort`, `ProxyUsername`, `ProxyTelnetCommand`; session key names are %XX-unescaped.

Rust types:

```rust
struct Session {
    id: String,             // uuid
    name: String,
    path: Vec<String>,      // container path segments (decoded)
    username: String,
    password: Option<Secret>,   // vault-encrypted if set
    private_key: Option<PathBuf>, // .ppk (converted) or OpenSSH
    host: String,
    port: u16,              // default 22
    certificate: Option<PathBuf>,
    launch_tool: Option<String>, // RDP/VNC client etc. (v0.2)
    x11_forward: bool,      // sX11
    copy_files_protocol: String, // cfProt, "sftp"
    proxy: Option<Proxy>,   // Socks4/5, Http, Local(telnet cmd)
    extra_args: Vec<String>, // passthrough to ssh
}
struct Proxy { kind: ProxyKind, host: String, port: u16, user: Option<String>, telnet_cmd: Option<String> }
```

## Storage

- `~/.config/nuxsshterm/connections.xml` — the session tree in WinSSHTerm format. **Passwords are
  never written to disk**: `save_tree`/`export_connections_file` strip `Node.password` before
  serialising (see `model::strip_passwords`). Passwords live in the encrypted vault (below) while
  unlocked, and only in memory when the vault is locked.
- `~/.config/nuxsshterm/log` — append-only backend error log (see "Error handling" below).
- `~/.config/nuxsshterm/vault.bin` — the encrypted secret blob (see "Vault" below).
- `~/.config/nuxsshterm/settings.json` — NuxSSHTerm's own **non-secret** UI-local settings:
  the dedicated `ssh-agent` record (`agent_socket` + `agent_pid`, Phase 4) and the per-session
  `ForwardAgent` flags keyed by vault-style path key. Kept out of the WinSSHTerm XML on purpose.

## Vault (Phase 1, shipped in v0.2)

- **On-disk format** (`vault.bin`): `[magic "NXVAULT1"][16-byte salt][12-byte nonce][AES-256-GCM
  ciphertext]`. The plaintext is JSON `{ sessions_with_passwords, imported_ppk_map }` where
  `sessions_with_passwords` maps a session path key (ancestor names joined by `/`) to its password,
  and `imported_ppk_map` holds `.ppk → .pem` conversions recorded by the import wizard (Phase 2).
- **Key derivation**: `Argon2id(master password, salt, m=64 MiB, t=3, p=4)` → 32-byte AES key
  (`src-tauri/src/vault.rs`). The salt is embedded in the file header so the key can be re-derived
  on unlock. The master password is **never stored**.
- **Lifecycle**: first run → "set master password" wizard (`vault_init`); every start → unlock
  dialog (`vault_unlock`); `vault_lock` drops the in-memory key/salt/blob; `vault_reset` (forgot
  password) wipes the vault file — session passwords are lost, connections survive.
- **In-memory key retention**: while unlocked the derived AES key + salt are held in `VaultState`,
  so re-encrypting after a password edit (`vault_put_password`) does not require re-entering the
  master password. The key is zeroized on lock/reset.
- **Frontend**: `useVault` hook owns the lifecycle; on unlock it hydrates connection passwords into
  the tree (`applyPasswords`), on lock it clears them (`clearPasswords`). Password edits in the
  Configuration panel are written to the vault via `vault_put_password`. The status bar shows
  `vault: unset | locked | unlocked`.

## Importer (Phase 2, shipped in v0.2.1)

`File → Import…` opens the **import wizard**, which sniffs the file and drives the whole flow:

1. **Format detection** (`src-tauri/src/importcmd.rs`): `.kdbx` (binary; needs `keepassxc-cli`),
   then content sniffing — `<?xml`/`<WinSSHTerm`/`<Node` → WinSSHTerm XML; otherwise PuTTY.
   Text is decoded from UTF-8 (with/without BOM) or UTF-16LE/BE (regedit's native encoding).
2. **WinSSHTerm `connections.xml` / `.settings`** — quick-xml parser for the Node tree above
   (tolerant: unknown attrs ignored, nested Containers, base64 decode errors fall back to raw
   name). Passwords are separated into the vault payload and stripped from the preview/save.
3. **PuTTY `.reg`** (`regedit /e putty.reg HKCU\Software\SimonTatham\PuTTY`) and **KiTTY `.txt`**
   (`Session\<path>\key=value` lines) — `src-tauri/src/putty.rs`. Handles `%XX` unescape of
   session names, builds folder containers from backslash paths, maps
   `HostName/PortNumber/UserName/PublicKeyFile/DetachedCertificate/X11Forwarding` and the proxy
   set (`ProxyMethod` 1=SOCKS4, 2=SOCKS5, 3=HTTP, 5=Local, `ProxyHost/Port/Username/ProxyTelnetCommand`),
   and skips `WinSSHTerm`/`WinSSHTerm_ScriptRunner` sessions.
4. **`.kdbx` via `keepassxc-cli export -f xml`** (`src-tauri/src/kdbx.rs`) — maps group titles →
   folders and entry `Title/URL/UserName/Password` → sessions. Entries without a URL are skipped
   (warned); passwords are returned for the vault. Password-protected databases produce a
   guidance warning (use KeePassXC's manual XML export).
5. **Wizard UI** (`src/components/ImportWizard.tsx`): path + **Scan** → preview tree with
   checkboxes, per-session overrides (host/user/port/key) and a **warnings** banner (absolute
   Windows `C:\…` key paths, unsupported proxy methods, skipped kdbx entries). **Import**
   converts every `.ppk` key via `puttygen <key>.ppk -O private-openssh -o <cfg>/imported/<stem>.pem`
   (originals untouched, output chmod 600), then **merges** into the native store (or replaces it),
   saves, and — while the vault is unlocked — stores passwords (`vault_put_password`) and records
   conversions in the vault's `imported_ppk_map` (`vault_put_ppk_import`).

## Terminal engine

`portable-pty::openpty()` → spawn `ssh`:

```
ssh [-p port] [-l user] [-i key] [-X|-Y] [-o ForwardAgent=yes] [-J jump]
    [-o "ProxyCommand=..."] [extra_args] host
```

- PTY master → xterm.js via IPC event (tauri `invoke`/events); xterm.js input → PTY stdin.
- **Binary-safe streaming**: PTY output is read in 8 KiB chunks and emitted as **base64** in the
  `pty-data` event (not `String::from_utf8_lossy`), so multi-byte UTF-8 sequences split across
  chunk boundaries are never corrupted. The frontend decodes base64 → `Uint8Array` → `term.write`.
- Resize: xterm fit addon → `pty.resize(cols, rows)`.
- Tab strip = app state; sessions survive tab close (reconnect button) — v0.1: close = kill.
- Host key prompt: xterm shows ssh's own prompt (pass-through) — https://
  known_hosts handled by system ssh natively.

## SFTP commander (WinSCP "commander view" equivalent)

One persistent `sftp` child per commander tab. The child is spawned **without
`-b`** (piped stdin keeps it alive command-after-command — verified on OpenSSH
9.x: it echoes every command as `sftp> <cmd>` on stdout and stays resident).
Each request is framed with a sentinel: after the real command the driver
pushes `!printf 'NXSFTPEND_<session-token>\n'` (a local shell command), and the
response is everything on stdout up to that sentinel plus stderr lines from the
same window. Errors do **not** abort the stream, so a long-lived child is safe
for sequential scripted ops (`src-tauri/src/sftp.rs`).

- `ls -la <abs path>` → parse columns `perms nlink owner group size mon day
  time|year name...` (name = tokens ≥9 joined with spaces). OpenSSH sftp prints
  **full paths**, so each `SftpEntry` carries `path` (full, op-ready) and
  `name` (basename for display; symlinks show the pre-` -> ` part).
- Remote ops (`sftp_op`): `get/put [-r]`, `rename`, `mkdir`, `rmdir`, `rm`,
  `chmod`, `chown` (numeric uid — sftp rejects names), `symlink`, and recursive
  delete. OpenSSH sftp **has no `rm -r`**, so recursive delete is a depth-first
  walk over the same child (`rm` files, `rmdir` dirs, symlinks removed as
  links — never followed).
- Local pane: `tokio` fs (`local_list`/`local_mkdir`/`local_rmdir`/`local_rm`/
  `local_rename`/`local_remove`). `local_remove` decides from
  `symlink_metadata` so symlinks are never followed. mtimes are formatted
  in-process (civil-date conversion, no chrono dep; UTC, close enough for a
  file manager).
- **sftp argv** differs from ssh: port is `-P` (not `-p`), user is `-o User=`
  (`-l` means bandwidth-limit in sftp), no X11 flag — `LaunchSpec::sftp_args()`.
- **Password auth**: when a session has a vault password and no key, the spawn
  sets `SSH_ASKPASS` to a 0700 helper in the config dir that cats a 0600
  per-session password file (`SSH_ASKPASS_REQUIRE=force`, `DISPLAY=:0`);
  password file is removed on tab close. Key/agent auth is inherited from the
  environment like normal.
- **Copy Files menu** per session (right-click "Copy Files" / Navigate → Copy
  Files): opens the commander on that host pre-navigated to `$HOME` (from the
  `pwd` handshake), honoring `cfProt=sftp` (other protocols toast an
  "unsupported" notice).
- Transfer engine: sequential scripted ops via the persistent child (v0.1).
  Byte-level progress/resume/parallel land with russh-sftp (Phase 6). The
  progress strip is file-level: current op text + an indeterminate indicator.
- Commander tabs share the tab strip with terminals; the remote/local pane
  listing state lives in the frontend (`CommanderView.tsx`) and refreshes on
  tab activation.
- Smoke test: `sftp::tests::smoke_localhost_driver` (`#[ignore]`d, run with
  `cargo test -- --ignored`) drives the full op battery against a throwaway
  local `nux-sftp-test` user over real sshd, with both password (askpass) and
  key auth.

## Key manager ("Pageant", shipped in v0.4.0)

`src-tauri/src/agent.rs` + `Tools → SSH key manager…` modal
(`src/components/KeyManagerModal.tsx`).

- **Socket detection**: if `SSH_AUTH_SOCK` is already set (desktop keyring, shell-started
  agent, …) the app adopts it (`present=true, ours=false`) and never touches it — no Start/Stop.
  If unset, **Start agent** spawns a dedicated `ssh-agent -a <config>/agent.sock` in daemon
  mode (verified on OpenSSH 9.x: the launcher exits immediately after printing the
  `SSH_AUTH_SOCK`/`SSH_AGENT_PID` env lines and the agent keeps running, Pageant-like, so keys
  survive app restarts). The record (`agent_socket` + `agent_pid`) is persisted in
  `settings.json`; next launch re-attaches to the same daemon. **Stop agent** runs
  `ssh-agent -k` with both env vars set (kills only our daemon) and clears the record.
- **Export to children**: `agent::configured_socket()` is applied by `pty.rs` (`pty_open`) and
  `sftp.rs` (`open`) as `SSH_AUTH_SOCK` on every child process, so terminal and SFTP sessions
  use agent keys (and `ForwardAgent=yes`, below) even when the app was started from a desktop
  launcher without the variable.
- **Panel**: lists identities from `ssh-add -l` (bits, fingerprint, comment, type), with
  **Add key…** (native file picker → `ssh-add <key>`), **Remove** per row and **Remove all**
  (`ssh-add -D`). Passphrase-protected keys prompt in the modal; the passphrase is fed through
  an askpass helper (`agent-askpass.sh`, 0700, mirroring the sftp helper) with
  `SSH_ASKPASS_REQUIRE=force`. `ssh-add -d` needs a key **path**, so the backend records the
  path against the fingerprint at add time (`fingerprint_of` via `ssh-keygen -lf`) — rows
  added by other tools show “—” for remove.
- **Forward Agent**: per-session `Node.forward_agent` toggle in the Configuration panel,
  mapped to `-o ForwardAgent=yes` by `LaunchSpec::ssh_args()`/`sftp_args()`. It is a UI-local
  field persisted in `settings.json` (never in the WinSSHTerm XML).
- **.ppk**: converted by the Phase 2 importer (`puttygen -O private-openssh` into
  `<config>/imported/`); the key manager operates on the resulting OpenSSH paths.

## UI layout (WinSSHTerm parity)

```
+--------------------------------------------------------------+
| Menu bar: File Edit View Tools Help                           |
| Quick-launch bar (v0.2): [session buttons] [search]          |
+--------------------------------+-----------------------------+
| Session tree (left)            | Tab strip [host1][host2][+] |
|  - container (expandable)      |                             |
|  - connection (double-click)   |   xterm.js / commander      |
| Right-click: Connect, Copy     |                             |
|   Files, Edit, Duplicate,      |                             |
|   Delete, Export               |                             |
+--------------------------------+-----------------------------+
| Status bar: connection state | host | Ln/Col | mode          |
+--------------------------------------------------------------+
```

- Windows-like theme (title bar colors, tab styling) via CSS; follows system dark/light (v0.2).
- App ID `com.alatcerdas.nuxsshterm`; `.desktop` entry; tray icon (v0.2).

## Error handling

- Frontend: a toast/notification stack (`useToasts` + `ToastStack`) replaces ad-hoc `notice`
  strings. Status/error messages are pushed as auto-expiring, dismissible toasts; the status bar
  keeps the latest line.
- Backend: command errors are logged to `~/.config/nuxsshterm/log` via `log::error` (append-only,
  best-effort, never panics).

## Tests & CI

- Rust unit tests: `xml.rs` round-trip (parse → write → parse, idempotency, escaping),
  `ssh.rs::ssh_args()` snapshot tests for port/user/key, X11/agent, SOCKS5/Local proxy, extra args,
  and `vault.rs` encrypt/decrypt round-trip, wrong-password rejection, and non-vault-data rejection,
  and `agent.rs` unit tests for `ssh-add -l` parsing + `SSH_AGENT_PID` extraction.
- Agent driver smoke test: `agent::tests::smoke_localhost_agent` (`#[ignore]` — run with
  `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored`) spawns a real `ssh-agent`
  on a throwaway socket, adds a throwaway key, and exercises add / list / remove-one /
  remove-all / `ssh-agent -k` (passed 2026-10-06).
- CI (`.github/workflows/ci.yml`): on Linux — `npm ci && npm run build` (frontend) and
  `cargo check` + `cargo clippy -- -D warnings` + `cargo test` (backend, with Tauri system deps).

## Security notes

- Master password never persisted; vault is zero-knowledge under Argon2id+AES-GCM.
- Keys remain on disk; only fingerprints live in the agent.
- No password in argv or logs; `sftp`/`ssh` use agent or vault-injected password via askpass pipe.
- Passwords are never written to `connections.xml` (stripped on save/export); they live in the
  encrypted vault while unlocked and only in memory when locked.
- .reg/.settings import: warn on absolute Windows paths (map to converted keys interactively).

## Migration checklist (for Sam, when the Windows box is available)

1. WinSSHTerm → copy `%APPDATA%\WinSSHTerm\WinSSHTerm.settings` (or File → export connections).
2. PuTTY → `regedit /e putty-sessions.reg HKCU\Software\SimonTatham\PuTTY`.
3. Keys → collect `.ppk` (+ any OpenSSH `.pem`) files.
4. (optional) KeePass `.kdbx` for password import.
5. Run importer wizard on Omarchy Quattro → review mapping → done.

## Roadmap

- **v0.1**: 4 must-haves + session tree/tabs (shipped).
- **v0.2**: encrypted vault (shipped in v0.2.0), PuTTY `.reg`/`.txt` + `.ppk` import and the
  import wizard (shipped in v0.2.1), SFTP commander (shipped in v0.3.0), key manager
  (shipped in v0.4.0).
- **v0.3**: quick-launch bar, tray, search/filter, russh in-process, jump-host editor, `.kdbx`
  (best-effort via keepassxc-cli).
- **v0.4**: scripts/automation (LaunchTools-style), multi-tab session grouping, session sync.