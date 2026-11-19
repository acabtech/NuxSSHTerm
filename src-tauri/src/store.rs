//! On-disk persistence: native store is the WinSSHTerm `connections.xml` format itself,
//! which means settings import/export is a straight round-trip. A secondary
//! `settings.json` holds NuxSSHTerm's own non-secret, UI-local settings that must
//! survive restarts but do not belong in the WinSSHTerm schema (agent record,
//! per-session ForwardAgent flags).

use crate::model::{KIND_CONNECTION, Node};
use crate::xml;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("nuxsshterm");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn connections_path() -> PathBuf {
    config_dir().join("connections.xml")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

fn default_cursor_style() -> String {
    "block".to_string()
}

fn default_font_family() -> String {
    "\"JetBrains Mono\", ui-monospace, \"DejaVu Sans Mono\", Consolas, monospace".to_string()
}

fn default_bell_style() -> String {
    "visual".to_string()
}

/// Terminal rendering preferences (xterm.js), persisted so they survive restarts.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalSettings {
    /// xterm.js `fontSize` in CSS pixels.
    pub font_size: u32,
    /// xterm.js `fontFamily` (CSS font-family string; issue #16).
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// xterm.js `scrollback` (buffer lines kept per pane).
    pub scrollback: u32,
    /// xterm.js `cursorBlink`.
    pub cursor_blink: bool,
    /// xterm.js `cursorStyle` ("block" | "underline" | "bar").
    #[serde(default = "default_cursor_style")]
    pub cursor_style: String,
    /// Bell behavior on terminal BEL: "none" | "visual" | "sound" | "both".
    #[serde(default = "default_bell_style")]
    pub bell_style: String,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_size: 13,
            font_family: default_font_family(),
            scrollback: 10000,
            cursor_blink: true,
            cursor_style: default_cursor_style(),
            bell_style: default_bell_style(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// SSH authentication ordering (issue #8). Controls whether sessions prefer the
/// credential configured per connection (private key / vault password) or the
/// ssh-agent identities, and whether agent identities are offered as fallback
/// after a credential-first attempt fails.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthSettings {
    /// "credential_first" | "agent_first" | "agent_only"
    pub auth_order: String,
    /// Offer agent identities as a fallback after the session credential when
    /// "credential_first" is set and a session credential exists.
    /// false = never offer agent identities (credential only).
    pub agent_fallback: bool,
}

impl<'de> Deserialize<'de> for AuthSettings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            auth_order: String,
            #[serde(default)]
            agent_fallback: Option<bool>,
            /// Legacy timed field (issue #12): `agentFallbackSecs` was only ever
            /// consulted as on/off, so migrate any nonzero value to enabled.
            #[serde(default)]
            agent_fallback_secs: Option<u32>,
        }
        let raw = Raw::deserialize(d)?;
        let agent_fallback = raw
            .agent_fallback
            .or_else(|| raw.agent_fallback_secs.map(|s| s > 0))
            .unwrap_or(true);
        Ok(Self {
            auth_order: raw.auth_order,
            agent_fallback,
        })
    }
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            auth_order: "credential_first".to_string(),
            agent_fallback: true,
        }
    }
}

impl AuthSettings {
    /// Whether stored credentials (private key / vault password) must be
    /// suppressed entirely for this session. True only for `agent_only`,
    /// which promises "only ssh-agent identities (ignore stored credential)".
    pub fn suppresses_credentials(&self) -> bool {
        self.auth_order == "agent_only"
    }

    /// Effective policy for one session: whether to export `SSH_AUTH_SOCK` to
    /// the child and whether to pin identity selection with `IdentitiesOnly`.
    ///
    /// - `agent_only` / `agent_first`: always export the agent socket.
    /// - `credential_first` with a configured credential: export the agent
    ///   socket only when the fallback is enabled — ssh naturally tries the
    ///   explicit `-i` key / askpass password first and agent identities
    ///   after, which realises the staged fallback. With no fallback the
    ///   socket is withheld and `IdentitiesOnly=yes` pins the credential.
    /// - `credential_first` without a credential: nothing to prefer; behave
    ///   like `agent_first` (export the socket if one is configured).
    pub fn session_policy(&self, has_credential: bool) -> (bool, bool) {
        match self.auth_order.as_str() {
            "agent_only" => (true, false),
            "agent_first" => (true, false),
            _ => {
                if has_credential && self.agent_fallback {
                    (true, false)
                } else if has_credential {
                    (false, true)
                } else {
                    (true, false)
                }
            }
        }
    }
}

fn default_auth() -> AuthSettings {
    AuthSettings::default()
}

fn default_vault_auto_lock_minutes() -> u32 {
    0
}

/// File transfer preferences for the SFTP commander (v0.5.x modernization).
/// Everything here is non-secret and safe to store in plain `settings.json`.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferSettings {
    /// Prompt before overwriting an existing file/folder in the destination.
    #[serde(default = "default_true")]
    pub confirm_before_overwrite: bool,
    /// Prompt before deleting a file/folder.
    #[serde(default = "default_true")]
    pub confirm_before_delete: bool,
    /// Initial local working directory for the commander; empty falls back to $HOME.
    pub default_local_path: String,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            confirm_before_overwrite: true,
            confirm_before_delete: true,
            default_local_path: String::new(),
        }
    }
}

/// Connections sidebar layout preferences (v0.5.1): expanded width and
/// collapse behavior.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct SidebarSettings {
    /// Expanded sidebar width in px (drag-resizable), clamped 200–560.
    pub width: u32,
    /// "hover" collapses after `collapse_delay_ms` on pointer-leave; "never"
    /// pins the sidebar open.
    pub collapse_mode: String,
    /// Hover-out collapse delay in ms (0 = collapse immediately).
    pub collapse_delay_ms: u32,
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self {
            width: 320,
            collapse_mode: "hover".to_string(),
            collapse_delay_ms: 0,
        }
    }
}

fn default_false() -> bool {
    false
}

/// The v0.5.0 modernized UI chrome settings (design-system tweaks + terminal).
/// Everything here is non-secret and safe to store in plain `settings.json`.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct UiSettings {
    /// "comfortable" | "compact" — chrome density.
    pub density: String,
    /// UI font scale multiplier (0.9–1.15).
    pub font_scale: f64,
    /// Show the configuration block under the connections rail.
    pub show_config: bool,
    /// Primary accent color as a CSS hex string (e.g. "#3b82f6").
    pub accent_color: String,
    /// Auto-lock the encrypted vault after this many minutes of inactivity
    /// while unlocked. 0 disables auto-lock.
    #[serde(default = "default_vault_auto_lock_minutes")]
    pub vault_auto_lock_minutes: u32,
    /// Default for new connections: forward the local ssh-agent to the remote host.
    #[serde(default)]
    pub default_forward_agent: bool,
    /// Default for new connections: private key path (e.g. ~/.ssh/id_ed25519).
    #[serde(default)]
    pub default_private_key: String,
    /// Terminal rendering preferences.
    pub terminal: TerminalSettings,
    /// SFTP commander / file transfer preferences.
    pub transfer: TransferSettings,
    /// Connections sidebar (width / collapse behavior).
    #[serde(default = "default_sidebar")]
    pub sidebar: SidebarSettings,
    /// SSH authentication ordering / agent fallback (issue #8).
    #[serde(default = "default_auth")]
    pub auth: AuthSettings,
    /// Opt-in: restore the last workspace (open tabs + pane layout) on start
    /// (issue #15). Default off for backward-compatible "fresh start".
    #[serde(default = "default_false")]
    pub restore_tabs: bool,
}

fn default_sidebar() -> SidebarSettings {
    SidebarSettings::default()
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            density: "comfortable".to_string(),
            font_scale: 1.0,
            show_config: true,
            accent_color: "#3b82f6".to_string(),
            vault_auto_lock_minutes: default_vault_auto_lock_minutes(),
            default_forward_agent: false,
            default_private_key: String::new(),
            terminal: TerminalSettings::default(),
            transfer: TransferSettings::default(),
            sidebar: SidebarSettings::default(),
            auth: AuthSettings::default(),
            restore_tabs: false,
        }
    }
}

/// NuxSSHTerm's own non-secret settings (kept out of the WinSSHTerm XML).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Serialized last-workspace snapshot (tabs + layout) for issue #15
    /// restore. Never contains vault passwords or secrets.
    #[serde(default)]
    pub workspace: Option<serde_json::Value>,
    /// Socket path of the dedicated `ssh-agent` this app spawned ("" = none).
    #[serde(default)]
    pub agent_socket: String,
    /// PID of that agent, needed by `ssh-agent -k` on stop ("" = unknown).
    #[serde(default)]
    pub agent_pid: String,
    /// UI-local per-session ForwardAgent flags, keyed by the same path key the
    /// vault uses (ancestor names joined by "/").
    #[serde(default)]
    pub forward_agent: HashMap<String, bool>,
    /// Modernized UI chrome preferences (v0.5.0).
    #[serde(default)]
    pub ui: UiSettings,
}

pub fn load_settings() -> Settings {
    if let Ok(bytes) = std::fs::read(settings_path())
        && let Ok(s) = serde_json::from_slice::<Settings>(&bytes)
    {
        return s;
    }
    Settings::default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let bytes = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(), bytes).map_err(|e| e.to_string())
}

/// The persisted dedicated-agent record (socket, pid). Empty when unset.
pub fn agent_record() -> (String, String) {
    let s = load_settings();
    (s.agent_socket, s.agent_pid)
}

/// Load the modernized UI chrome settings (v0.5.0).
pub fn get_ui_settings() -> UiSettings {
    load_settings().ui
}

/// Persist the modernized UI chrome settings, preserving the other settings.
pub fn set_ui_settings(ui: &UiSettings) -> Result<(), String> {
    let mut s = load_settings();
    s.ui = ui.clone();
    // Clamp sanity-sensitive values regardless of what the caller sent.
    s.ui.sidebar.width = s.ui.sidebar.width.clamp(200, 560);
    s.ui.sidebar.collapse_delay_ms = s.ui.sidebar.collapse_delay_ms.min(20_000);
    save_settings(&s)
}

/// Load the last saved workspace snapshot, if any (issue #15).
pub fn get_workspace() -> Option<serde_json::Value> {
    load_settings().workspace
}

/// Persist the workspace snapshot (issue #15).
pub fn set_workspace(v: serde_json::Value) -> Result<(), String> {
    let mut s = load_settings();
    s.workspace = Some(v);
    save_settings(&s)
}

/// Persist the dedicated-agent record (empty strings clear it).
pub fn set_agent_record(socket: &str, pid: &str) {
    let mut s = load_settings();
    s.agent_socket = socket.to_string();
    s.agent_pid = pid.to_string();
    let _ = save_settings(&s);
}

/// Path key for a connection: ancestor names joined by "/" (vault scheme).
fn path_key_of(path: &[String]) -> String {
    path.join("/")
}

/// Collect per-connection ForwardAgent flags into a settings map.
fn collect_forward_agent(nodes: &[Node]) -> HashMap<String, bool> {
    let mut out: HashMap<String, bool> = HashMap::new();
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    crate::model::flatten(nodes, &empty, &mut flat);
    for (path, n) in flat {
        if n.node_type == KIND_CONNECTION && n.forward_agent {
            out.insert(path_key_of(&path), true);
        }
    }
    out
}

/// Apply settings-stored ForwardAgent flags onto a freshly loaded tree.
fn apply_forward_agent(nodes: &mut [Node]) {
    let flags = load_settings().forward_agent;
    apply_forward_agent_flags(nodes, &flags, &mut Vec::new());
}

/// Walk the real tree in place (no clones) and assign each connection's flag
/// from the persisted map. `flatten` clones nodes, so mutating its output never
/// reached the tree the caller returned.
fn apply_forward_agent_flags(
    nodes: &mut [Node],
    flags: &HashMap<String, bool>,
    prefix: &mut Vec<String>,
) {
    for n in nodes.iter_mut() {
        prefix.push(n.name.clone());
        if n.node_type == KIND_CONNECTION {
            n.forward_agent = flags.get(&path_key_of(prefix)).copied().unwrap_or(false);
        }
        apply_forward_agent_flags(&mut n.children, flags, prefix);
        prefix.pop();
    }
}

/// Load the session tree; returns an empty tree on first run.
pub fn load_tree() -> Vec<Node> {
    let mut nodes = if let Ok(s) = std::fs::read_to_string(connections_path())
        && let Ok(nodes) = xml::parse_connections(&s)
    {
        nodes
    } else {
        Vec::new()
    };
    apply_forward_agent(&mut nodes);
    nodes
}

pub fn save_tree(nodes: &[Node]) -> Result<(), String> {
    // Never persist plaintext passwords — strip them before writing to disk.
    std::fs::write(connections_path(), export_xml(nodes)).map_err(|e| e.to_string())?;
    // Persist UI-local ForwardAgent flags in settings.json (not the WinSSHTerm XML).
    let mut s = load_settings();
    s.forward_agent = collect_forward_agent(nodes);
    let _ = save_settings(&s);
    Ok(())
}

/// Serialise a tree for saving/export with every password removed.
///
/// Vault passwords must never leave the encrypted vault: both `save_tree` and
/// the user-facing export use this function.
pub fn export_xml(nodes: &[Node]) -> String {
    let mut clean = nodes.to_vec();
    crate::model::strip_passwords(&mut clean);
    xml::write_connections(&clean)
}

/// Import a WinSSHTerm `connections.xml` / `.settings` file into the native store.
pub fn import_connections_xml(xml_text: &str) -> Result<Vec<Node>, String> {
    let nodes = xml::parse_connections(xml_text)?;
    if nodes.is_empty() {
        return Err("no sessions found in file".into());
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_xml_never_contains_passwords() {
        let mut node = Node::connection("Prod", "prod.example", "deploy", 22);
        node.password = "super-secret".into();
        let tree = vec![node];

        let exported = export_xml(&tree);

        assert!(exported.contains("prod.example"));
        assert!(exported.contains("deploy"));
        assert!(
            !exported.contains("super-secret"),
            "export must strip passwords"
        );
        assert!(
            exported.contains("Password=\"\""),
            "password attribute must be blank in exports"
        );
    }

    #[test]
    fn forward_agent_flags_apply_to_real_tree() {
        let mut nodes = vec![
            Node::container("group", true).with_children(vec![Node::connection(
                "nested",
                "nested.example",
                "u",
                22,
            )]),
            Node::connection("top", "top.example", "u", 22),
        ];
        let mut flags = HashMap::new();
        flags.insert("group/nested".to_string(), true);
        flags.insert("top".to_string(), true);

        apply_forward_agent_flags(&mut nodes, &flags, &mut Vec::new());

        assert!(
            nodes[0].children[0].forward_agent,
            "nested flag must reach the returned tree"
        );
        assert!(
            nodes[1].forward_agent,
            "top-level flag must reach the returned tree"
        );
    }

    #[test]
    fn forward_agent_flags_default_to_false() {
        let mut nodes = vec![Node::connection("top", "top.example", "u", 22)];
        nodes[0].forward_agent = true;

        apply_forward_agent_flags(&mut nodes, &HashMap::new(), &mut Vec::new());

        assert!(!nodes[0].forward_agent);
    }

    #[test]
    fn collect_and_apply_use_the_same_path_key() {
        let mut nodes = vec![
            Node::container("group", true).with_children(vec![Node::connection(
                "nested",
                "nested.example",
                "u",
                22,
            )]),
        ];
        nodes[0].children[0].forward_agent = true;
        let flags = collect_forward_agent(&nodes);

        assert_eq!(flags.get("group/nested"), Some(&true));

        let mut loaded = vec![
            Node::container("group", true).with_children(vec![Node::connection(
                "nested",
                "nested.example",
                "u",
                22,
            )]),
        ];
        apply_forward_agent_flags(&mut loaded, &flags, &mut Vec::new());
        assert!(loaded[0].children[0].forward_agent);
    }

    #[test]
    fn agent_only_suppresses_credentials() {
        let auth = AuthSettings {
            auth_order: "agent_only".into(),
            agent_fallback: true,
        };
        assert!(auth.suppresses_credentials());
    }

    #[test]
    fn credential_fallback_bool_matches_legacy_secs_semantics() {
        // Issue #12: the runtime only ever treated the timed fallback as on/off.
        let on: AuthSettings =
            serde_json::from_str(r#"{"authOrder":"credential_first","agentFallbackSecs":5}"#)
                .unwrap();
        assert!(on.agent_fallback);
        assert_eq!(on.session_policy(true), (true, false));

        let off: AuthSettings =
            serde_json::from_str(r#"{"authOrder":"credential_first","agentFallbackSecs":0}"#)
                .unwrap();
        assert!(!off.agent_fallback);
        assert_eq!(off.session_policy(true), (false, true));
    }
}
