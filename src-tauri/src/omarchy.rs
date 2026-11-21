//! Optional Omarchy "Remote Hosts" integration (issue #23).
//!
//! Omarchy's Quickshell menu reads a single JSONC file at
//! `~/.config/omarchy/extensions/omarchy-menu.jsonc`. Rows are keyed by dotted
//! ids, so `remote-hosts.nuxsshterm.<id>` lands inside the existing "Remote
//! Hosts" submenu. We own only the lines between the managed markers; every
//! other line in the user's file is preserved byte-for-byte.

use crate::model::{self, KIND_CONNECTION, Node};
use crate::store::{self, UiSettings};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const BEGIN: &str = "// BEGIN NuxSSHTerm managed favorites (do not edit)";
const END: &str = "// END NuxSSHTerm managed favorites";
const PARENT_ID: &str = "remote-hosts.nuxsshterm";

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OmarchyStatus {
    pub available: bool,
    pub menu_path: String,
    pub published: usize,
}

pub fn menu_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("omarchy")
        .join("extensions")
        .join("omarchy-menu.jsonc")
}

/// Graceful detection: an installed Omarchy config is enough; no hard failure
/// on other desktops. The feature is additionally gated by an opt-in setting.
pub fn available() -> bool {
    if let Some(parent) = menu_path().parent()
        && parent.exists()
    {
        return true;
    }
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("omarchy")
}

pub fn status() -> OmarchyStatus {
    let path = menu_path();
    let published = std::fs::read_to_string(&path)
        .map(|text| text.lines().filter(|l| l.trim_start().starts_with('"') && l.contains(PARENT_ID) && !l.trim_start().starts_with(&format!("\"{PARENT_ID}\""))).count())
        .unwrap_or(0);
    OmarchyStatus {
        available: available(),
        menu_path: path.to_string_lossy().to_string(),
        published,
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Single-quote a shell argument the way a careful human would.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in value.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Build the managed JSONC block from currently favorite connections.
/// Returns an empty string when there is nothing to publish, so stale rows are
/// removed on unfavorite/rename/delete as well as on first enable.
fn render_block(tree: &[Node]) -> (String, usize) {
    let mut flat = Vec::new();
    model::flatten(tree, &[], &mut flat);
    let favorites: Vec<(String, Node)> = flat
        .into_iter()
        .filter(|(_, node)| node.node_type == KIND_CONNECTION && node.favorite)
        .map(|(path, node)| (path.join("/"), node))
        .collect();
    if favorites.is_empty() {
        return (String::new(), 0);
    }

    let mut lines = vec![
        BEGIN.to_string(),
        format!(
            "  \"{PARENT_ID}\": {{\"icon\":\"󰒕\",\"label\":\"NuxSSHTerm\",\"description\":\"Favorite SSH sessions\"}},"
        ),
    ];
    for (key, node) in &favorites {
        let id = format!("{PARENT_ID}.{}-{}", slug(&node.name), short_hash(key));
        let label = if node.name.trim().is_empty() {
            key.clone()
        } else {
            node.name.clone()
        };
        let description = format!(
            "{}@{}:{} · NuxSSHTerm",
            if node.username.is_empty() { "?" } else { &node.username },
            if node.hostname.is_empty() { "?" } else { &node.hostname },
            if node.port.is_empty() { "22" } else { &node.port },
        );
        let action = format!("nuxsshterm --session {}", shell_quote(key));
        lines.push(format!(
            "  {}: {{",
            json_string(&id)
        ));
        lines.push("    \"icon\":\"󰒕\",".to_string());
        lines.push(format!("    \"label\":{},", json_string(&label)));
        lines.push(format!("    \"description\":{},", json_string(&description)));
        lines.push(format!("    \"action\":{},", json_string(&action)));
        lines.push("  },".to_string());
    }
    lines.push(END.to_string());
    (lines.join("\n"), favorites.len())
}

/// Replace an existing managed block, or insert a fresh one just after the
/// top-level `{`. Inserting at the top avoids fragile trailing-comma surgery on
/// the user's own entries.
fn with_managed_block(text: &str, block: &str) -> String {
    if let (Some(start), Some(end)) = (text.find(BEGIN), text.find(END)) {
        let end = end + END.len();
        let mut out = String::with_capacity(text.len() + block.len());
        out.push_str(&text[..start]);
        out.push_str(block);
        out.push_str(&text[end..]);
        return out;
    }
    let Some(open) = text.find('{') else {
        return text.to_string();
    };
    if block.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + block.len());
    out.push_str(&text[..=open]);
    out.push('\n');
    out.push_str(block);
    out.push('\n');
    out.push_str(text[open + 1..].trim_start_matches(['\r', '\n']));
    out
}

fn write_into(path: &Path, tree: &[Node]) -> Result<usize, String> {
    let (block, count) = render_block(tree);
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{\n}\n".to_string(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let next = with_managed_block(&existing, &block);
    if next != existing {
        store::atomic_write(path, next.as_bytes())
            .map_err(|e| format!("cannot update {}: {e}", path.display()))?;
    }
    Ok(count)
}

fn remove_from(path: &Path) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let next = with_managed_block(&text, "");
    if next != text {
        store::atomic_write(path, next.as_bytes())
            .map_err(|e| format!("cannot update {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Publish the current favorites, or remove our rows when the option is off.
pub fn sync(tree: &[Node]) -> Result<usize, String> {
    if !available() {
        return Ok(0);
    }
    write_into(&menu_path(), tree)
}

pub fn sync_for_settings(ui: &UiSettings) -> Result<usize, String> {
    if !ui.omarchy_menu_integration {
        remove_from(&menu_path())?;
        return Ok(0);
    }
    sync(&store::load_tree())
}

/* -------- `nuxsshterm --session <path>` launch handoff -------- */

fn pending_session_path() -> PathBuf {
    store::config_dir().join("pending-session")
}

pub fn set_pending_session(path_key: &str) -> Result<(), String> {
    if path_key.trim().is_empty() {
        return Ok(());
    }
    store::atomic_write(&pending_session_path(), path_key.trim().as_bytes())
}

pub fn take_pending_session() -> Option<String> {
    let path = pending_session_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let key = text.trim().to_string();
    if key.is_empty() { None } else { Some(key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn favorite(name: &str, host: &str) -> Node {
        let mut node = Node::connection(name, host, "deploy", 22);
        node.favorite = true;
        node
    }

    #[test]
    fn block_only_contains_favorite_connections() {
        let mut fav = favorite("Prod", "prod.example");
        let plain = Node::connection("Dev", "dev.example", "dev", 22);
        fav.children.push(Node::connection("Nested", "n.example", "n", 22));
        let tree = vec![Node::container("grp", true).with_children(vec![fav, plain])];

        let (block, count) = render_block(&tree);
        assert_eq!(count, 1);
        assert!(block.contains("\"label\":\"Prod\""));
        assert!(block.contains("nuxsshterm --session 'grp/Prod'"));
        assert!(!block.contains("\"label\":\"Dev\""));
        assert!(block.contains(&short_hash("grp/Prod")));
    }

    #[test]
    fn managed_block_is_replaced_and_user_rows_survive() {
        let temp = std::env::temp_dir().join(format!(
            "nx-omarchy-menu-{}-{}.jsonc",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let original = "{\n  // mine\n  \"remote-hosts\": {\"label\":\"Remote Hosts\"},\n}\n";
        std::fs::write(&temp, original).unwrap();

        let tree = vec![favorite("A'B", "host.example")];
        write_into(&temp, &tree).unwrap();
        let first = std::fs::read_to_string(&temp).unwrap();
        assert!(first.contains("// mine"));
        // JSON escaping doubles the shell quote's literal backslash on disk.
        assert!(first.contains("nuxsshterm --session 'A'\\\\''B'"));

        write_into(&temp, &[]).unwrap();
        let cleared = std::fs::read_to_string(&temp).unwrap();
        assert!(!cleared.contains(BEGIN));
        assert!(cleared.contains("// mine"));
        assert!(cleared.contains("\"remote-hosts\""));

        std::fs::remove_file(&temp).unwrap();
    }
}
