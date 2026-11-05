//! Import commands for the Phase 2 wizard (sessions + keys).
//!
//! Sniffs the input format (WinSSHTerm XML, PuTTY `.reg`, KiTTY `.txt`,
//! KeePass `.kdbx`) and returns a preview plus warnings; converts `.ppk` keys
//! via `puttygen` at import time (originals are never modified).

use crate::kdbx;
use crate::model::{Node, KIND_CONNECTION};
use crate::putty;
use crate::wsst_crypto;
use crate::xml;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// true when the file holds encrypted passwords that could not be unlocked
    /// (the UI should ask for the WinSSHTerm master password and rescan)
    pub needs_master_password: bool,
    /// number of encrypted password values found in the source
    pub encrypted_passwords: usize,
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

#[derive(Default)]
struct PasswordHarvest {
    passwords: HashMap<String, String>,
    needs_master_password: bool,
    encrypted_count: usize,
    warnings: Vec<String>,
}

/// Collect non-empty connection passwords with their `/`-joined vault key.
fn raw_passwords(nodes: &[Node]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    crate::model::flatten(nodes, &empty, &mut flat);
    for (path, n) in flat {
        if n.node_type == KIND_CONNECTION && !n.password.is_empty() {
            out.push((join_slash(&path), n.password.clone()));
        }
    }
    out
}

/// Turn WinSSHTerm's encrypted `Password` attributes into plaintext.
///
/// WinSSHTerm encrypts connection passwords with random keys stored in a `key`
/// file, itself encrypted with the WinSSHTerm master password. Passing `None`
/// for `master_password` only works when no custom master password was set.
fn harvest_passwords(
    nodes: &[Node],
    key_file: Option<&[u8]>,
    verify_key: Option<&str>,
    master_password: Option<&str>,
) -> PasswordHarvest {
    let raw = raw_passwords(nodes);
    if raw.is_empty() {
        return PasswordHarvest::default();
    }

    let mut out = PasswordHarvest {
        encrypted_count: raw.len(),
        ..Default::default()
    };

    let Some(key_file) = key_file else {
        // Plain connections.xml without a sibling `key` file: old exports can
        // contain plaintext passwords, but encrypted-looking values cannot be
        // recovered without the key file.
        let encrypted: Vec<&(String, String)> = raw
            .iter()
            .filter(|(_, value)| wsst_crypto::looks_encrypted(value))
            .collect();
        if !encrypted.is_empty() {
            out.warnings.push(format!(
                "{} password value(s) look encrypted, but no WinSSHTerm `key` file was found next to the imported file — passwords were skipped.",
                encrypted.len()
            ));
            out.encrypted_count = encrypted.len();
            return out;
        }
        out.passwords = raw.into_iter().collect();
        out.encrypted_count = 0;
        return out;
    };

    let keys = wsst_crypto::decrypt_key_file(key_file, master_password);
    let (password_key, salt_key) = match keys {
        Ok(keys) => keys,
        Err(e) => {
            out.needs_master_password = true;
            out.warnings.push(if master_password.is_some() {
                format!(
                    "Could not unlock the WinSSHTerm key file with that master password ({e}). Check the password and rescan."
                )
            } else {
                format!(
                    "This WinSSHTerm backup contains {} encrypted password(s). Enter the WinSSHTerm master password to decrypt them ({e}).",
                    raw.len()
                )
            });
            return out;
        }
    };

    if let Some(verify_key) = verify_key
        && let Err(e) = wsst_crypto::verify_key_pair(&password_key, &salt_key, verify_key)
    {
        out.needs_master_password = true;
        out.warnings.push(if master_password.is_some() {
            format!("The WinSSHTerm master password is incorrect ({e}).")
        } else {
            format!("The WinSSHTerm master password is required to decrypt passwords ({e}).")
        });
        return out;
    }

    let mut failed = 0usize;
    for (path, cipher) in raw {
        match wsst_crypto::decrypt_value(&cipher, &password_key, &salt_key) {
            Ok(plain) => {
                out.passwords.insert(path, plain);
            }
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        out.warnings.push(format!(
            "{failed} password value(s) could not be decrypted and were skipped."
        ));
    }
    out
}

/// WinSSHTerm stores the key file as `key` next to its `connections.xml`.
fn sibling_key_file(path: &str) -> Option<Vec<u8>> {
    let key_path = Path::new(path).with_file_name("key");
    std::fs::read(key_path).ok()
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
fn run_import_file(path: String, master_password: Option<&str>) -> Result<ImportPreview, String> {
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
            needs_master_password: false,
            encrypted_passwords: 0,
        });
    }

    // ---- text formats ----
    let text = decode_text(&bytes);
    let t = text.trim();

    let (nodes, passwords, warnings, needs_master_password, encrypted_passwords, format) =
        if t.contains("<WinSSHTerm_Backup") {
            // ---- WinSSHTerm compressed backup (<Data><c n=".."> base64(gzip(pb))) ----
            let envelope = xml::parse_backup_envelope(t)?;
            let harvest = harvest_passwords(
                &envelope.nodes,
                envelope.key_file.as_deref(),
                envelope.verify_key.as_deref(),
                master_password,
            );
            let mut n = envelope.nodes;
            crate::model::strip_passwords(&mut n);
            let mut wn = xml_warnings(&n);
            wn.extend(harvest.warnings);
            (
                n,
                harvest.passwords,
                wn,
                harvest.needs_master_password,
                harvest.encrypted_count,
                "WinSSHTerm backup (compressed)".into(),
            )
        } else if t.starts_with("<?xml")
            || t.starts_with("<WinSSHTerm")
            || t.starts_with("<Node")
            || t.contains("<WinSSHTerm")
        {
            // ---- WinSSHTerm connections.xml / .settings ----
            let mut n = xml::parse_connections(t)?;
            if n.is_empty() {
                return Err("no sessions found in the WinSSHTerm file".into());
            }
            // A plain connections.xml keeps its key file as `key` alongside it.
            let key_file = sibling_key_file(&path);
            let harvest = harvest_passwords(&n, key_file.as_deref(), None, master_password);
            crate::model::strip_passwords(&mut n);
            let mut wn = xml_warnings(&n);
            wn.extend(harvest.warnings);
            (
                n,
                harvest.passwords,
                wn,
                harvest.needs_master_password,
                harvest.encrypted_count,
                "WinSSHTerm XML".into(),
            )
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
            (n, HashMap::new(), w, false, 0, fmt)
        };

    let count = count_connections(&nodes);
    Ok(ImportPreview {
        format,
        count,
        nodes,
        passwords,
        warnings,
        needs_master_password,
        encrypted_passwords,
    })
}

/* ------------------------------ commands ------------------------------ */

#[tauri::command]
pub fn import_sessions_file(
    path: String,
    master_password: Option<String>,
) -> Result<ImportPreview, String> {
    run_import_file(path, master_password.as_deref())
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
        let res = run_import_file("/nonexistent/nope.reg".into(), None);
        assert!(res.is_err());
    }

    /// Optional end-to-end check against a real WinSSHTerm export. Set
    /// `WINSSHTERM_BACKUP=/path/to/WinSSHTerm.xml` to run it; otherwise it is a
    /// no-op so the suite stays portable. Set `WINSSHTERM_MASTER_PASSWORD` when
    /// the backup is protected by a WinSSHTerm master password.
    #[test]
    fn imports_real_backup_from_env() {
        let Ok(path) = std::env::var("WINSSHTERM_BACKUP") else {
            return;
        };
        let master = std::env::var("WINSSHTERM_MASTER_PASSWORD").ok();
        let preview =
            run_import_file(path, master.as_deref()).expect("real backup should import");
        assert!(preview.count > 0, "expected at least one session");
        assert!(
            preview.format.contains("backup"),
            "expected the compressed-backup format label, got {}",
            preview.format
        );
        assert!(
            preview.encrypted_passwords > 0,
            "real backup should contain encrypted passwords"
        );
        if master.is_some() {
            assert!(
                !preview.needs_master_password,
                "supplied master password should unlock the backup"
            );
            assert!(
                !preview.passwords.is_empty(),
                "decrypted passwords should be present for the vault"
            );
        } else {
            assert!(
                preview.passwords.is_empty(),
                "ciphertext must never be stored as a password"
            );
            assert!(preview.needs_master_password);
        }
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
        n2.private_key = "/home/user/.ssh/id_ed25519".into();
        assert!(xml_warnings(&[n2]).is_empty());
    }
}
