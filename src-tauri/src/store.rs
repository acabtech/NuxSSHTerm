//! On-disk persistence: native store is the WinSSHTerm `connections.xml` format itself,
//! which means settings import/export is a straight round-trip. A secondary
//! `settings.json` holds NuxSSHTerm's own non-secret, UI-local settings that must
//! survive restarts but do not belong in the WinSSHTerm schema (agent record,
//! per-session ForwardAgent flags).

use crate::model::{Node, KIND_CONNECTION};
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

/// NuxSSHTerm's own non-secret settings (kept out of the WinSSHTerm XML).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
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
}

pub fn load_settings() -> Settings {
    if let Ok(bytes) = std::fs::read(settings_path())
        && let Ok(s) = serde_json::from_slice::<Settings>(&bytes) {
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
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    crate::model::flatten(nodes, &empty, &mut flat);
    for (path, mut n) in flat {
        if n.node_type == KIND_CONNECTION {
            n.forward_agent = match flags.get(&path_key_of(&path)) {
                Some(&v) => v,
                None => false,
            };
        }
    }
}

/// Load the session tree; on first run, seed it with the tree reconstructed from
/// Sam's WinSSHTerm screenshot and persist that so it can be edited immediately.
pub fn load_tree() -> Vec<Node> {
    let mut nodes = if let Ok(s) = std::fs::read_to_string(connections_path())
        && let Ok(nodes) = xml::parse_connections(&s)
            && !nodes.is_empty() {
                nodes
            } else {
                let seed = crate::seed::default_tree();
                let _ = save_tree(&seed);
                seed
            };
    apply_forward_agent(&mut nodes);
    nodes
}

pub fn save_tree(nodes: &[Node]) -> Result<(), String> {
    // Never persist plaintext passwords — strip them before writing to disk.
    let mut clean = nodes.to_vec();
    crate::model::strip_passwords(&mut clean);
    std::fs::write(connections_path(), xml::write_connections(&clean)).map_err(|e| e.to_string())?;
    // Persist UI-local ForwardAgent flags in settings.json (not the WinSSHTerm XML).
    let mut s = load_settings();
    s.forward_agent = collect_forward_agent(nodes);
    let _ = save_settings(&s);
    Ok(())
}

/// Import a WinSSHTerm `connections.xml` / `.settings` file into the native store.
pub fn import_connections_xml(xml_text: &str) -> Result<Vec<Node>, String> {
    let nodes = xml::parse_connections(xml_text)?;
    if nodes.is_empty() {
        return Err("no sessions found in file".into());
    }
    Ok(nodes)
}