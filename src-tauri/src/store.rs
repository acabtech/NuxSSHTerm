//! On-disk persistence: native store is the WinSSHTerm `connections.xml` format itself,
//! which means settings import/export is a straight round-trip.

use crate::model::Node;
use crate::xml;
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

#[allow(dead_code)] // reserved for non-secret settings (v0.2)
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Load the session tree; on first run, seed it with the tree reconstructed from
/// Sam's WinSSHTerm screenshot and persist that so it can be edited immediately.
pub fn load_tree() -> Vec<Node> {
    if let Ok(s) = std::fs::read_to_string(connections_path()) {
        if let Ok(nodes) = xml::parse_connections(&s) {
            if !nodes.is_empty() {
                return nodes;
            }
        }
    }
    let seed = crate::seed::default_tree();
    let _ = save_tree(&seed);
    seed
}

pub fn save_tree(nodes: &[Node]) -> Result<(), String> {
    // Never persist plaintext passwords — strip them before writing to disk.
    let mut clean = nodes.to_vec();
    crate::model::strip_passwords(&mut clean);
    std::fs::write(connections_path(), xml::write_connections(&clean)).map_err(|e| e.to_string())
}

/// Import a WinSSHTerm `connections.xml` / `.settings` file into the native store.
pub fn import_connections_xml(xml_text: &str) -> Result<Vec<Node>, String> {
    let nodes = xml::parse_connections(xml_text)?;
    if nodes.is_empty() {
        return Err("no sessions found in file".into());
    }
    Ok(nodes)
}