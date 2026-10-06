//! NuxSSHTerm — Tauri backend.

mod importcmd;
mod kdbx;
mod local_fs;
mod log;
mod model;
mod pty;
mod putty;
mod seed;
mod sftp;
mod ssh;
mod store;
mod vault;
mod xml;

use model::Node;

#[tauri::command]
fn load_tree() -> Vec<Node> {
    store::load_tree()
}

#[tauri::command]
fn save_tree(tree: Vec<Node>) -> Result<(), String> {
    store::save_tree(&tree).inspect_err(|e| {
        log::error("save_tree", e);
    })
}

#[tauri::command]
fn config_dir() -> String {
    store::config_dir().to_string_lossy().to_string()
}

/// Parse a WinSSHTerm `connections.xml` / exported `.settings` file for preview/import.
#[tauri::command]
fn import_connections_file(path: String) -> Result<Vec<Node>, String> {
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {path}: {e}"))
        .inspect_err(|e| {
            log::error("import_connections_file", e);
        })?;
    store::import_connections_xml(&text).inspect_err(|e| {
        log::error("import_connections_file", e);
    })
}

/// Write the current tree back out in WinSSHTerm format (round-trip export).
#[tauri::command]
fn export_connections_file(path: String, tree: Vec<Node>) -> Result<(), String> {
    // Never write plaintext passwords to an exported file either.
    let mut clean = tree;
    model::strip_passwords(&mut clean);
    std::fs::write(&path, xml::write_connections(&clean))
        .map_err(|e| e.to_string())
        .inspect_err(|e| {
            log::error("export_connections_file", e);
        })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(pty::PtyState::default())
        .manage(vault::VaultState::default())
        .manage(sftp::SftpState::default())
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
            vault::vault_status,
            vault::vault_init,
            vault::vault_unlock,
            vault::vault_lock,
            vault::vault_reset,
            vault::vault_get_passwords,
            vault::vault_put_password,
            vault::vault_remove_password,
            vault::vault_get_ppk_map,
            vault::vault_put_ppk_import,
            importcmd::import_sessions_file,
            importcmd::puttygen_available,
            importcmd::convert_ppk,
            kdbx::keepassxc_available,
            sftp::sftp_open,
            sftp::sftp_close,
            sftp::sftp_list,
            sftp::sftp_op,
            local_fs::home_dir,
            local_fs::local_list,
            local_fs::local_mkdir,
            local_fs::local_rmdir,
            local_fs::local_rm,
            local_fs::local_rename,
            local_fs::local_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}