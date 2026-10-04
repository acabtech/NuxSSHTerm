//! NuxSSHTerm — Tauri backend.

mod model;
mod pty;
mod seed;
mod ssh;
mod store;
mod xml;

use model::Node;

#[tauri::command]
fn load_tree() -> Vec<Node> {
    store::load_tree()
}

#[tauri::command]
fn save_tree(tree: Vec<Node>) -> Result<(), String> {
    store::save_tree(&tree)
}

#[tauri::command]
fn config_dir() -> String {
    store::config_dir().to_string_lossy().to_string()
}

/// Parse a WinSSHTerm `connections.xml` / exported `.settings` file for preview/import.
#[tauri::command]
fn import_connections_file(path: String) -> Result<Vec<Node>, String> {
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    store::import_connections_xml(&text)
}

/// Write the current tree back out in WinSSHTerm format (round-trip export).
#[tauri::command]
fn export_connections_file(path: String, tree: Vec<Node>) -> Result<(), String> {
    std::fs::write(&path, xml::write_connections(&tree)).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(pty::PtyState::default())
        .invoke_handler(tauri::generate_handler![
            load_tree,
            save_tree,
            config_dir,
            import_connections_file,
            export_connections_file,
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            pty::pty_alive,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}