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
        && let Ok(nodes) = xml::parse_connections(&s) {
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
        assert!(!exported.contains("super-secret"), "export must strip passwords");
        assert!(
            exported.contains("Password=\"\""),
            "password attribute must be blank in exports"
        );
    }

    #[test]
    fn forward_agent_flags_apply_to_real_tree() {
        let mut nodes = vec![
            Node::container("group", true).with_children(vec![
                Node::connection("nested", "nested.example", "u", 22),
            ]),
            Node::connection("top", "top.example", "u", 22),
        ];
        let mut flags = HashMap::new();
        flags.insert("group/nested".to_string(), true);
        flags.insert("top".to_string(), true);

        apply_forward_agent_flags(&mut nodes, &flags, &mut Vec::new());

        assert!(nodes[0].children[0].forward_agent, "nested flag must reach the returned tree");
        assert!(nodes[1].forward_agent, "top-level flag must reach the returned tree");
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
            Node::container("group", true).with_children(vec![
                Node::connection("nested", "nested.example", "u", 22),
            ]),
        ];
        nodes[0].children[0].forward_agent = true;
        let flags = collect_forward_agent(&nodes);

        assert_eq!(flags.get("group/nested"), Some(&true));

        let mut loaded = vec![
            Node::container("group", true).with_children(vec![
                Node::connection("nested", "nested.example", "u", 22),
            ]),
        ];
        apply_forward_agent_flags(&mut loaded, &flags, &mut Vec::new());
        assert!(loaded[0].children[0].forward_agent);
    }
}