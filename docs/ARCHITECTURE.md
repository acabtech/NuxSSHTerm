# NuxSSHTerm — Architecture (v0.1)

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

- `~/.config/nuxsshterm/config.json` — non-secret settings + session tree (no passwords).
- `~/.config/nuxsshterm/vault.bin` — secret blob: JSON {sessions_with_passwords, imported_ppk_map}
  encrypted with **AES-256-GCM**; key = **Argon2id**(master password, random 16-byte salt,
  m=64 MiB, t=3, p=4). Master password is never stored. First run → "set master password" wizard;
  every start → unlock dialog (WinSSHTerm behaviour). Forgot password → reset vault (session
  passwords are wiped; connections survive).

## Importer (built against real schema; user's files arrive later)

1. `WinSSHTerm.settings` / `connections.xml` — XML parser for the Node tree above (tolerant:
   unknown attrs ignored, nested Containers, base64 decode errors fall back to raw name).
2. PuTTY `.reg` export (`regedit /e putty.reg HKCU\Software\SimonTatham\PuTTY`) + also accepts
   `.txt` (KiTTY-style `Key\value` lines). Handles `%XX` unescape, skips `WinSSHTerm`/`WinSSHTerm_ScriptRunner`.
3. (v0.2) `.kdbx` via `keepassxc-cli export` — map titles+URLs to hosts.
4. Wizard UI: file pickers → preview tree → per-item mapping (host/user/port/key) → import into
   native store; .ppk files converted on the spot via puttygen → `~/.ssh/`.

## Terminal engine

`portable-pty::openpty()` → spawn `ssh`:

```
ssh [-p port] [-l user] [-i key] [-X|-Y] [-o ForwardAgent=yes] [-J jump]
    [-o "ProxyCommand=..."] [extra_args] host
```

- PTY master → xterm.js via IPC event (tauri `invoke`/events); xterm.js input → PTY stdin.
- Resize: xterm fit addon → `pty.resize(cols, rows)`.
- Tab strip = app state; sessions survive tab close (reconnect button) — v0.1: close = kill.
- Host key prompt: xterm shows ssh's own prompt (pass-through) — https://
  known_hosts handled by system ssh natively.

## SFTP commander (WinSCP "commander view" equivalent)

One persistent `sftp` child per commander tab, batch mode (stdin commands, no prompts, parse stdout):

- `ls -la <abs path>` → parse columns: `perms nlink owner group size mon day time|year name...`
  (name = tokens ≥9 joined with spaces) → dual-pane render.
- Remote ops: `get/put [-r]`, `rename`, `mkdir`, `rmdir`, `rm -r`, `chmod`, `chown`, `symlink`.
- Local pane: tokio fs. Path bars on both panes; Ctrl+U swaps panes; Ctrl+R refresh; F5 copy,
  F6 move, F7 mkdir, F8 delete, F9 properties, Ctrl+T new tab, Alt+F4 close.
- **Copy Files menu** per session (WinSSHTerm's right-click "Copy Files"): opens commander on that
  host (cfProt=sftp), remote pane pre-navigated to $HOME; keeps transfer progress in a bottom strip.
- Transfer engine: scripted sequential ops via the sftp child (v0.1); resume/parallel via russh-sftp (v0.2).

## Key manager ("Pageant")

- Detects `SSH_AUTH_SOCK` (ssh-agent or GNOME Keyring); if unset, offers to spawn a dedicated
  `ssh-agent` and export the socket to child sessions.
- UI: list (`ssh-add -l` fingerprints incl. comment), add (`ssh-add <key>`; passphrase prompt via
  PTY/askpass helper), remove all (`ssh-add -D`), remove one (`ssh-add -d <key>`).
- .ppk: `puttygen <key.ppk> -O private-openssh -o <basename>` (v3 ppk supported by putty 0.83),
  key import = conversion + agent add; original file never modified.
- Per-session `ForwardAgent` toggle (Pageant-style agent forwarding for jump hosts).

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

## Security notes

- Master password never persisted; vault is zero-knowledge under Argon2id+AES-GCM.
- Keys remain on disk; only fingerprints live in the agent.
- No password in argv or logs; `sftp`/`ssh` use agent or vault-injected password via askpass pipe.
- .reg/.settings import: warn on absolute Windows paths (map to converted keys interactively).

## Migration checklist (for Sam, when the Windows box is available)

1. WinSSHTerm → copy `%APPDATA%\WinSSHTerm\WinSSHTerm.settings` (or File → export connections).
2. PuTTY → `regedit /e putty-sessions.reg HKCU\Software\SimonTatham\PuTTY`.
3. Keys → collect `.ppk` (+ any OpenSSH `.pem`) files.
4. (optional) KeePass `.kdbx` for password import.
5. Run importer wizard on Omarchy Quattro → review mapping → done.

## Roadmap

- **v0.1**: 4 must-haves + session tree/tabs (this doc).
- **v0.2**: quick-launch bar, tray, search/filter, russh in-process, jump-host editor, kdbx, export round-trip.
- **v0.3**: scripts/automation (LaunchTools-style), multi-tab session grouping, session sync.