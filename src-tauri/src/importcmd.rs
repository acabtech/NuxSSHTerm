//! Import commands for the Phase 2 wizard (sessions + keys).
//!
//! Sniffs the input format (WinSSHTerm XML, PuTTY `.reg`, KiTTY `.txt`,
//! KeePass `.kdbx`) and returns a preview plus warnings; converts `.ppk` keys
//! via `puttygen` at import time (originals are never modified).

use crate::kdbx;
use crate::model::{Node, KIND_CONNECTION};
use crate::putty;
use crate::xml;
use std::collections::HashMap;
use std::path::PathBuf;

/// Preview returned by `import_sessions_file` for the import wizard.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportPreview {
    /// human-readable source format label
    pub format: String,
    /// number of session (connection) nodes in `nodes`
    pub count: usize,
    /// preview tree — passwords already stripped (they travel in `passwords`)
    pub nodes: Vec<Node>,
    /// path key (ancestor names joined by `/`) -> password, for the vault
    pub passwords: HashMap<String, String>,
    /// warnings surfaced while parsing (Windows paths, skipped entries, …)
    pub warnings: Vec<String>,
}

/// Result of a `.ppk` → OpenSSH conversion.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ConvertedKey {
    pub original: String,
    pub converted: String,
    /// true when the target `.pem` already existed (no puttygen run)
    pub was_already: bool,
}

/// True when `name` resolves to a file on `PATH`.
pub fn program_on_path(name: &str) -> bool {
    let path_var = std::env::var("PATH").unwrap_or_default();
    for entry in std::env::split_paths(&path_var) {
        if entry.join(name).is_file() {
            return true;
        }
    }
    false
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        // UTF-8 BOM
        String::from_utf8_lossy(&bytes[3..]).to_string()
    } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16LE (regedit's native export encoding)
        String::from_utf16le_lossy(&bytes[2..])
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        String::from_utf16be_lossy(&bytes[2..])
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

fn join_slash(parts: &[String]) -> String {
    let mut out = String::with_capacity(0);
    let mut first = true;
    for p in parts {
        if !first {
            out.push('/');
        }
        out.push_str(p);
        first = false;
    }
    out
}

fn count_connections(nodes: &[Node]) -> usize {
    let mut n = 0;
    for node in nodes {
        if node.node_type == KIND_CONNECTION {
            n += 1;
        }
        n += count_connections(&node.children);
    }
    n
}

/// Collect node passwords keyed by their `/`-joined path (the vault key scheme).
fn collect_passwords(nodes: &[Node]) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    crate::model::flatten(nodes, &empty, &mut flat);
    for (path, n) in flat {
        if n.node_type == KIND_CONNECTION && !n.password.is_empty() {
            out.insert(join_slash(&path), n.password.clone());
        }
    }
    out
}

fn xml_warnings(nodes: &[Node]) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    crate::model::flatten(nodes, &empty, &mut flat);
    for (path, n) in flat {
        let pathstr = join_slash(&path);
        if putty::is_windows_path(&n.private_key) {
            warnings.push(format!(
                "Session '{pathstr}': PrivateKey is a Windows path — set the Linux key path after import."
            ));
        }
        if putty::is_windows_path(&n.certificate) {
            warnings.push(format!(
                "Session '{pathstr}': Certificate is a Windows path — set the Linux certificate path after import."
            ));
        }
    }
    warnings
}

/// Parse any supported import file into a preview.
fn run_import_file(path: String) -> Result<ImportPreview, String> {
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("cannot read {path}: {e}"))
        .inspect_err(|e| {
            crate::log::error("import_sessions_file", e);
        })?;

    let pb = PathBuf::from(&path);
    let ext = pb.extension().map(|o| o.to_string_lossy().to_string()).unwrap_or_default();

    // ---- KeePass .kdbx (binary; needs keepassxc-cli) ----
    if ext.eq_ignore_ascii_case("kdbx") {
        if !kdbx::keepassxc_available() {
            return Err(
                "This is a KeePass database, but keepassxc-cli is not installed. \
Install keepassxc-cli and try again, or export the database as XML in KeePassXC \
(Menu → Database → Export → XML) and import that file instead."
                    .into(),
            );
        }
        let xml_text = kdbx::export_xml(&path)?;
        let (nodes, passwords, warnings) = kdbx::parse_xml(&xml_text)?;
        let count = count_connections(&nodes);
        return Ok(ImportPreview {
            format: "KeePass (.kdbx)".to_string(),
            count,
            nodes,
            passwords,
            warnings,
        });
    }

    // ---- text formats ----
    let text = decode_text(&bytes);
    let t = text.trim();

    let (nodes, passwords, warnings, format) = if t.starts_with("<?xml")
        || t.starts_with("<WinSSHTerm")
        || t.starts_with("<Node")
        || t.contains("<WinSSHTerm") {
            // ---- WinSSHTerm connections.xml / .settings ----
            let mut n = xml::parse_connections(t)?;
            if n.is_empty() {
                return Err("no sessions found in the WinSSHTerm file".into());
            }
            let p = collect_passwords(&n);
            crate::model::strip_passwords(&mut n);
            let wn = xml_warnings(&n);
            (n, p, wn, "WinSSHTerm XML".into())
        } else {
            // ---- PuTTY .reg / KiTTY .txt ----
            let (n, w) = putty::import_text(&text);
            if n.is_empty() {
                return Err(
                    "No PuTTY sessions found — unrecognised file format. Expected a PuTTY \
.reg export (regedit /e \"HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\") or a KiTTY .txt export."
                        .into(),
                );
            }
            let fmt = if t.to_ascii_uppercase().contains("WINDOWS REGISTRY EDITOR") {
                "PuTTY .reg".into()
            } else {
                "KiTTY .txt".into()
            };
            (n, HashMap::new(), w, fmt)
        };

    let count = count_connections(&nodes);
    Ok(ImportPreview {
        format,
        count,
        nodes,
        passwords,
        warnings,
    })
}

/* ------------------------------ commands ------------------------------ */

#[tauri::command]
pub fn import_sessions_file(path: String) -> Result<ImportPreview, String> {
    run_import_file(path)
}

#[tauri::command]
pub fn puttygen_available() -> bool {
    program_on_path("puttygen")
}

/// Convert a PuTTY `.ppk` private key to OpenSSH `.pem` (original untouched).
#[tauri::command]
pub fn convert_ppk(source: String) -> Result<ConvertedKey, String> {
    convert_ppk_impl(source)
}

fn convert_ppk_impl(source: String) -> Result<ConvertedKey, String> {
    if !program_on_path("puttygen") {
        return Err(
            "puttygen is not installed — install putty-tools (puttygen) to convert .ppk keys.".into(),
        );
    }
    let src = PathBuf::from(&source);
    if !src.exists() {
        return Err(format!("key file not found on this machine: {source}"));
    }

    let stem = src
        .file_stem()
        .map(|o| o.to_string_lossy().to_string())
        .unwrap_or("key".into());
    let dir = crate::store::config_dir().join("imported");
    let _ = std::fs::create_dir_all(&dir);
    let out_path = dir.join(format!("{stem}.pem"));
    let converted = out_path.to_string_lossy().to_string();

    if out_path.exists() {
        // Already converted on an earlier import.
        return Ok(ConvertedKey {
            original: source.clone(),
            converted,
            was_already: true,
        });
    }

    let mut cmd = std::process::Command::new("puttygen");
    cmd.arg(&source);
    cmd.arg("-O");
    cmd.arg("private-openssh");
    cmd.arg("-o");
    cmd.arg(&converted);
    let out = cmd
        .output()
        .map_err(|e| format!("puttygen could not be started: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        let err2 = String::from_utf8_lossy(&out.stdout).into_owned();
        return Err(format!("puttygen failed: {err}{err2}"));
    }

    // Converted keys are secrets — never leave them world-readable.
    let mut ch = std::process::Command::new("chmod");
    ch.arg("600");
    ch.arg(&converted);
    let _ = ch.output();

    Ok(ConvertedKey {
        original: source,
        converted,
        was_already: false,
    })
}
/* ------------------------------ tests ------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::putty;

    /// Encode a string as UTF-16LE with a BOM (regedit's export encoding).
    fn utf16le_bom(s: &str) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.push(0xFF);
        out.push(0xFE);
        for c in s.encode_utf16() {
            out.push(c as u8);
            out.push((c >> 8) as u8);
        }
        out
    }

    #[test]
    fn decodes_utf16le_regedit_export_end_to_end() {
        let reg = "[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\prod%20db]\n\"HostName\"=\"10.9.8.7\"\n\"PortNumber\"=\"2222\"\n\"UserName\"=\"deploy\"\n";
        let text = decode_text(&utf16le_bom(reg));
        assert!(!text.is_empty());
        let (nodes, warnings) = putty::import_text(&text);
        assert_eq!(nodes.len(), 1);
        assert_eq!((&nodes[0]).name, "prod db");
        assert_eq!((&nodes[0]).hostname, "10.9.8.7");
        assert_eq!((&nodes[0]).port, "2222");
        assert!(warnings.is_empty());
    }

    #[test]
    fn rejects_unreadable_path() {
        let res = run_import_file("/nonexistent/nope.reg".into());
        assert!(res.is_err());
    }

    #[test]
    fn xml_warnings_cover_private_key_and_certificate() {
        use crate::model::Node;
        // Both PrivateKey and Certificate are Windows paths → two warnings.
        let mut n = Node::connection("win host", "10.0.0.9", "sam", 22);
        n.private_key = "C:\\keys\\id_ed25519.ppk".into();
        n.certificate = "D:\\certs\\host.pem".into();
        let warns = xml_warnings(&[n]);
        assert_eq!(warns.len(), 2);
        assert!(warns[0].contains("PrivateKey"));
        assert!(warns[1].contains("Certificate"));

        // Linux paths → no warnings.
        let mut n2 = Node::connection("linux host", "10.0.0.10", "sam", 22);
        n2.private_key = "/home/sam/.ssh/id_ed25519".into();
        assert!(xml_warnings(&[n2]).is_empty());
    }
}
