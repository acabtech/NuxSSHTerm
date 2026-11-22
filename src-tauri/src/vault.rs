//! Encrypted vault for session passwords (Phase 1).
//!
//! Key  = Argon2id(master password, 16-byte salt, m=64 MiB, t=3, p=4).
//! Blob = AES-256-GCM(JSON `{ sessions_with_passwords, imported_ppk_map }`).
//! Stored at `~/.config/nuxsshterm/vault.bin`.
//!
//! The salt used for key derivation is embedded in the file header, so the key
//! can be re-derived from the master password on unlock. While unlocked we keep
//! the derived AES key (and its salt) in memory so re-encrypting after a
//! password edit does not require re-entering the master password. The master
//! password itself is never retained.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;
use zeroize::Zeroize;

const MAGIC: &[u8; 8] = b"NXVAULT1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// Domain separator for the API-key → key-encryption-key derivation (issue #22).
const AUTO_KEK_DOMAIN: &[u8] = b"nuxsshterm-auto-unlock-v1\0";
const DEFAULT_AUTO_KEY_ENV: &str = "NUXSSH_API_KEY";

/// `(salt, nonce, ciphertext)` slices borrowed from an on-disk vault file.
type VaultParts<'a> = (&'a [u8], &'a [u8], &'a [u8]);

/// Encrypted copy of the vault's AES key, unwrapped by an API key at unlock.
/// The master password is never stored; only this wrapped key is on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AutoUnlockConfig {
    version: u32,
    key_id: String,
    nonce: String,
    wrapped_key: String,
}

impl Default for AutoUnlockConfig {
    fn default() -> Self {
        Self {
            version: 1,
            key_id: String::new(),
            nonce: String::new(),
            wrapped_key: String::new(),
        }
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoUnlockStatus {
    pub configured: bool,
    pub key_id: String,
    pub key_registered: bool,
}

/// The plaintext payload stored (encrypted) in the vault.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultBlob {
    /// session path key -> password
    pub sessions_with_passwords: HashMap<String, String>,
    /// original .ppk path -> converted .pem path (populated by Phase 2 import)
    pub imported_ppk_map: HashMap<String, String>,
}

#[derive(Default)]
struct VaultInner {
    blob: VaultBlob,
    /// The AES-256 key, present only while the vault is unlocked.
    key: Option<[u8; KEY_LEN]>,
    /// The salt the key was derived from (embedded in the file header).
    salt: Option<[u8; SALT_LEN]>,
}

impl VaultInner {
    fn unlocked(&self) -> bool {
        self.key.is_some()
    }
}

/// Tauri-managed state for the vault.
#[derive(Default)]
pub struct VaultState(Mutex<VaultInner>);

#[derive(Clone, serde::Serialize)]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
}

pub fn vault_path() -> PathBuf {
    crate::store::config_dir().join("vault.bin")
}

pub fn is_initialized() -> bool {
    vault_path().exists()
}

fn auto_unlock_path() -> PathBuf {
    crate::store::config_dir().join("auto-unlock.json")
}

fn take_inner(state: &VaultState) -> std::sync::MutexGuard<'_, VaultInner> {
    state.0.lock().unwrap()
}

fn clear_inner(inner: &mut VaultInner) {
    inner.blob = VaultBlob::default();
    if let Some(mut k) = inner.key.take() {
        k.zeroize();
    }
    inner.salt = None;
}

/// Derive a 32-byte AES key from the master password + salt using Argon2id.
fn derive_key(master: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN], String> {
    let params = Params::new(64 * 1024, 3, 4, Some(KEY_LEN)).map_err(|e| e.to_string())?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(master, salt, &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

/// Encrypt the blob with an already-derived key + its salt and return the
/// on-disk layout: `[magic][salt][nonce][ciphertext]`.
fn encrypt_with_key(
    blob: &VaultBlob,
    key: &[u8; KEY_LEN],
    salt: &[u8; SALT_LEN],
) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let plaintext = serde_json::to_vec(blob).map_err(|e| e.to_string())?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Split an on-disk layout into `(salt, nonce, ciphertext)`.
fn parse_vault(data: &[u8]) -> Result<VaultParts<'_>, String> {
    let header = MAGIC.len() + SALT_LEN + NONCE_LEN;
    if data.len() < header || &data[..MAGIC.len()] != MAGIC {
        return Err("not a NuxSSHTerm vault file".into());
    }
    Ok((
        &data[MAGIC.len()..MAGIC.len() + SALT_LEN],
        &data[MAGIC.len() + SALT_LEN..header],
        &data[header..],
    ))
}

/// Decrypt a vault blob with an already-unwrapped key (master or API-key path).
fn decrypt_with_key(data: &[u8], key: &[u8; KEY_LEN]) -> Result<VaultBlob, String> {
    let (_, nonce, ct) = parse_vault(data)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "wrong master password or corrupted vault".to_string())?;
    serde_json::from_slice(&pt).map_err(|e| e.to_string())
}

/// Decrypt an on-disk layout back into the blob, the derived key, and its salt.
fn decrypt(data: &[u8], master: &[u8]) -> Result<(VaultBlob, [u8; KEY_LEN], [u8; SALT_LEN]), String> {
    let (salt_bytes, _, _) = parse_vault(data)?;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(salt_bytes);
    let key = derive_key(master, &salt)?;
    let blob = decrypt_with_key(data, &key)?;
    Ok((blob, key, salt))
}

/// Atomically write `bytes` to `path`: create a temp file in the same
/// directory (mode 0600 on Unix), write, fsync, then rename over `path`.
/// A crash or power loss mid-write can then never truncate/corrupt the
/// existing file — at worst a stale temp file is left behind.
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| "vault path has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "invalid vault path".to_string())?;
    let tmp = dir.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    let write = || -> Result<(), String> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Persist the current in-memory blob to disk using the retained key + salt.
fn save(state: &VaultState) -> Result<(), String> {
    let inner = state.0.lock().unwrap();
    let key = inner.key.ok_or("vault is locked")?;
    let salt = inner.salt.ok_or("vault is locked")?;
    let bytes = encrypt_with_key(&inner.blob, &key, &salt)?;
    atomic_write(&vault_path(), &bytes)
}

/// Load + decrypt the vault into memory, marking it unlocked.
fn load(state: &VaultState, master: &[u8]) -> Result<(), String> {
    let data = std::fs::read(vault_path()).map_err(|e| e.to_string())?;
    let (blob, key, salt) = decrypt(&data, master)?;
    let mut inner = take_inner(state);
    inner.blob = blob;
    inner.key = Some(key);
    inner.salt = Some(salt);
    Ok(())
}

/* ------------------- opt-in API-key auto-unlock (issue #22) ------------------- */

/// Domain-separated SHA-256 turns a bearer API key into a 256-bit KEK. The
/// plaintext key is never stored; only the AES-GCM-wrapped vault key is.
fn kek_from_api_key(key: &str) -> [u8; KEY_LEN] {
    let mut digest = Sha256::new();
    digest.update(AUTO_KEK_DOMAIN);
    digest.update(key.trim().as_bytes());
    let out = digest.finalize();
    let mut kek = [0u8; KEY_LEN];
    kek.copy_from_slice(&out);
    kek
}

fn seal_with_key(kek: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new_from_slice(kek).map_err(|e| e.to_string())?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|e| e.to_string())?;
    Ok((nonce.to_vec(), ct))
}

fn open_with_key(kek: &[u8; KEY_LEN], nonce: &[u8], ct: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(kek).map_err(|e| e.to_string())?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "auto-unlock key is wrong or the wrapped key is corrupted".into())
}

fn read_auto_config() -> Result<Option<AutoUnlockConfig>, String> {
    let path = auto_unlock_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("invalid auto-unlock configuration: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read auto-unlock configuration: {e}")),
    }
}

fn write_auto_config(cfg: &AutoUnlockConfig) -> Result<(), String> {
    let bytes = serde_json::to_vec(cfg).map_err(|e| e.to_string())?;
    atomic_write(&auto_unlock_path(), &bytes)
}

fn clear_auto_config() {
    match std::fs::remove_file(auto_unlock_path()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => crate::log::error("clear_auto_config", &e.to_string()),
    }
}

/// Turn the opt-in setting off in `settings.json`. Shared by explicit disable,
/// key revocation, and vault reset so no stale "enabled" flag survives.
fn disable_auto_unlock_settings() -> Result<(), String> {
    let mut settings = crate::store::load_settings();
    settings.ui.vault_auto_unlock_enabled = false;
    settings.ui.vault_auto_unlock_key_id.clear();
    settings.ui.vault_auto_unlock_source = "none".into();
    crate::store::save_settings(&settings)
}

fn expand_path(path: &str) -> PathBuf {
    if (path == "~" || path.starts_with("~/"))
        && let Some(home) = dirs::home_dir()
    {
        return home.join(path.trim_start_matches("~/"));
    }
    PathBuf::from(path)
}

/// Read the API key plaintext from the configured local-only source.
fn api_key_from_source(ui: &crate::store::UiSettings) -> Result<String, String> {
    if !ui.vault_auto_unlock_enabled {
        return Err("automatic vault unlock is disabled".into());
    }
    let key = match ui.vault_auto_unlock_source.as_str() {
        "env" => {
            let name = if ui.vault_auto_unlock_env_var.trim().is_empty() {
                DEFAULT_AUTO_KEY_ENV
            } else {
                ui.vault_auto_unlock_env_var.trim()
            };
            std::env::var(name)
                .map_err(|_| format!("environment variable {name} is not set"))?
        }
        "file" => {
            if ui.vault_auto_unlock_file_path.trim().is_empty() {
                return Err("auto-unlock key file path is empty".into());
            }
            let path = expand_path(ui.vault_auto_unlock_file_path.trim());
            std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read auto-unlock key file {}: {e}", path.display()))?
        }
        _ => return Err("no auto-unlock key source configured".into()),
    };
    let key = key.trim().to_string();
    if !key.starts_with(crate::apikeys::KEY_PREFIX) {
        return Err("auto-unlock key is not a NuxSSHTerm API key".into());
    }
    Ok(key)
}

/// Find the registered key record matching `key` (constant-time hash compare).
fn registered_key(key: &str) -> Result<crate::store::ApiKeyRecord, String> {
    let key = key.trim();
    crate::store::api_keys()
        .into_iter()
        .find(|record| crate::apikeys::verify(record, key))
        .ok_or_else(|| "API key is not registered (or was revoked)".into())
}

/// Verify configuration + key and recover the vault key without mutating state.
/// Returns the key id so both the Tauri command and the headless CLI can use it.
fn auto_unlock_key() -> Result<([u8; KEY_LEN], String), String> {
    let cfg = read_auto_config()?.ok_or("automatic vault unlock is not configured")?;
    if cfg.version != 1 {
        return Err("unsupported auto-unlock configuration version".into());
    }
    let ui = crate::store::get_ui_settings();
    let key = api_key_from_source(&ui)?;
    let record = registered_key(&key)?;
    if record.id != cfg.key_id {
        return Err("configured auto-unlock key was replaced or revoked".into());
    }
    let nonce = B64.decode(cfg.nonce.as_bytes()).map_err(|e| e.to_string())?;
    let wrapped = B64.decode(cfg.wrapped_key.as_bytes()).map_err(|e| e.to_string())?;
    let mut kek = kek_from_api_key(&key);
    let plain = open_with_key(&kek, &nonce, &wrapped);
    kek.zeroize();
    let plain = plain?;
    if plain.len() != KEY_LEN {
        return Err("unwrapped vault key has the wrong length".into());
    }
    let mut vault_key = [0u8; KEY_LEN];
    vault_key.copy_from_slice(&plain);
    Ok((vault_key, record.id))
}

/// Status for the settings UI — never exposes key material.
pub fn auto_unlock_status() -> AutoUnlockStatus {
    let configured = match read_auto_config() {
        Ok(cfg) => cfg.is_some(),
        Err(_) => false,
    };
    let key_id = read_auto_config()
        .ok()
        .flatten()
        .map(|c| c.key_id)
        .unwrap_or_default();
    let key_registered = !key_id.is_empty()
        && crate::store::api_keys()
            .iter()
            .any(|r| r.id == key_id);
    AutoUnlockStatus {
        configured,
        key_id,
        key_registered,
    }
}

/// Headless validation: proves the configured key can unwrap the vault key and
/// decrypt the blob, then drops all key material. Nothing is persisted/printed
/// except the key id by the CLI caller.
pub fn auto_unlock_headless() -> Result<String, String> {
    let (mut vault_key, id) = auto_unlock_key()?;
    let data = std::fs::read(vault_path()).map_err(|e| e.to_string())?;
    let result = decrypt_with_key(&data, &vault_key);
    vault_key.zeroize();
    result?;
    Ok(id)
}

/// If the revoked key was the auto-unlock key, discard the wrapped copy and
/// disable the setting so a stale wrapper cannot be reused later (issue #22).
pub fn on_api_key_revoked(id: &str) {
    let matches = matches!(read_auto_config(), Ok(Some(cfg)) if cfg.key_id == id);
    if !matches {
        return;
    }
    clear_auto_config();
    let _ = disable_auto_unlock_settings();
}

/* ------------------------------ commands ------------------------------ */

#[tauri::command]
pub fn vault_status(state: State<'_, VaultState>) -> VaultStatus {
    let inner = state.0.lock().unwrap();
    VaultStatus {
        initialized: is_initialized(),
        unlocked: inner.unlocked(),
    }
}

/// First-run: create the vault with a new master password.
#[tauri::command]
pub fn vault_init(state: State<'_, VaultState>, master: String) -> Result<(), String> {
    if is_initialized() {
        return Err("vault already initialized".into());
    }
    if master.is_empty() {
        return Err("master password cannot be empty".into());
    }
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let key = derive_key(master.as_bytes(), &salt)?;
    let blob = VaultBlob::default();
    let bytes = encrypt_with_key(&blob, &key, &salt)?;
    atomic_write(&vault_path(), &bytes)?;
    let mut inner = state.0.lock().unwrap();
    inner.blob = blob;
    inner.key = Some(key);
    inner.salt = Some(salt);
    Ok(())
}

/// Unlock the vault with the master password.
#[tauri::command]
pub fn vault_unlock(state: State<'_, VaultState>, master: String) -> Result<(), String> {
    load(&state, master.as_bytes())
}

/// Wrap the current vault key with a registered API key. The master password
/// is verified against the on-disk vault but never persisted.
#[tauri::command]
pub fn vault_auto_unlock_enable(
    _state: State<'_, VaultState>,
    master: String,
    key: String,
) -> Result<String, String> {
    let key = key.trim().to_string();
    let record = registered_key(&key)?;
    let data = std::fs::read(vault_path()).map_err(|e| e.to_string())?;
    let (_, mut vault_key, _) = decrypt(&data, master.as_bytes())?;

    let mut kek = kek_from_api_key(&key);
    let sealed = seal_with_key(&kek, &vault_key);
    kek.zeroize();
    let (nonce, wrapped) = sealed?;
    vault_key.zeroize();
    write_auto_config(&AutoUnlockConfig {
        version: 1,
        key_id: record.id.clone(),
        nonce: B64.encode(nonce),
        wrapped_key: B64.encode(wrapped),
    })?;

    // Deliberately do not mutate vault state: enabling the wrapper must not
    // silently unlock a locked vault or desync the frontend's status.
    Ok(record.id)
}

/// Remove the wrapped vault key. The master password flow remains unchanged.
#[tauri::command]
pub fn vault_auto_unlock_disable() -> Result<(), String> {
    clear_auto_config();
    disable_auto_unlock_settings()
}

#[tauri::command]
pub fn vault_auto_unlock_status() -> AutoUnlockStatus {
    auto_unlock_status()
}

/// Unlock using the configured API key instead of an interactive password.
#[tauri::command]
pub fn vault_unlock_auto(state: State<'_, VaultState>) -> Result<(), String> {
    let (vault_key, _) = auto_unlock_key()?;
    let data = std::fs::read(vault_path()).map_err(|e| e.to_string())?;
    let (salt_bytes, _, _) = parse_vault(&data)?;
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(salt_bytes);
    let blob = decrypt_with_key(&data, &vault_key)?;
    let mut inner = take_inner(&state);
    inner.blob = blob;
    inner.key = Some(vault_key);
    inner.salt = Some(salt);
    Ok(())
}

/// Lock the vault, dropping the in-memory key, salt, and blob.
#[tauri::command]
pub fn vault_lock(state: State<'_, VaultState>) -> Result<(), String> {
    let mut inner = take_inner(&state);
    clear_inner(&mut inner);
    Ok(())
}

/// Forgot password: wipe the vault (passwords lost, connections kept).
#[tauri::command]
pub fn vault_reset(state: State<'_, VaultState>) -> Result<(), String> {
    // Never report success while the encrypted vault is still on disk (#42).
    // A missing file is already the desired end state.
    match std::fs::remove_file(vault_path()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("failed to delete vault file: {e}")),
    }
    }
    clear_auto_config();
    disable_auto_unlock_settings()?;
    let mut inner = take_inner(&state);
    clear_inner(&mut inner);
    Ok(())
}

/// Return all stored session passwords (path key -> password).
#[tauri::command]
pub fn vault_get_passwords(
    state: State<'_, VaultState>,
) -> Result<HashMap<String, String>, String> {
    let inner = state.0.lock().unwrap();
    if !inner.unlocked() {
        return Err("vault is locked".into());
    }
    Ok(inner.blob.sessions_with_passwords.clone())
}

/// Store (or, when empty, remove) a session password and persist the vault.
#[tauri::command]
pub fn vault_put_password(
    state: State<'_, VaultState>,
    path: String,
    password: String,
) -> Result<(), String> {
    {
        let mut inner = state.0.lock().unwrap();
        if !inner.unlocked() {
            return Err("vault is locked".into());
        }
        if password.is_empty() {
            inner.blob.sessions_with_passwords.remove(&path);
        } else {
            inner.blob.sessions_with_passwords.insert(path, password);
        }
    }
    save(&state)
}

/// Remove a stored session password and persist the vault.
#[tauri::command]
pub fn vault_remove_password(state: State<'_, VaultState>, path: String) -> Result<(), String> {
    {
        let mut inner = state.0.lock().unwrap();
        if !inner.unlocked() {
            return Err("vault is locked".into());
        }
        inner.blob.sessions_with_passwords.remove(&path);
    }
    save(&state)
}

/// Return all recorded `.ppk` → `.pem` conversions (original path -> converted path).
#[tauri::command]
pub fn vault_get_ppk_map(state: State<'_, VaultState>) -> Result<HashMap<String, String>, String> {
    let inner = state.0.lock().unwrap();
    if !inner.unlocked() {
        return Err("vault is locked".into());
    }
    Ok(inner.blob.imported_ppk_map.clone())
}

/// Record a `.ppk` → `.pem` conversion (Phase 2 import) and persist the vault.
#[tauri::command]
pub fn vault_put_ppk_import(
    state: State<'_, VaultState>,
    original: String,
    converted: String,
) -> Result<(), String> {
    {
        let mut inner = state.0.lock().unwrap();
        if !inner.unlocked() {
            return Err("vault is locked".into());
        }
        inner.blob.imported_ppk_map.insert(original, converted);
    }
    save(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob_with(pw: &str) -> VaultBlob {
        let mut b = VaultBlob::default();
        b.sessions_with_passwords
            .insert("Proxmox Hosts/Elitedesk One".into(), pw.into());
        b
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let master = b"master-password";
        // Mirror the real flow: derive a key from the master password + a fresh
        // salt, encrypt, then decrypt using only the master password (the salt
        // is embedded in the file).
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_key(master, &salt).unwrap();
        let blob = blob_with("s3cret");
        let bytes = encrypt_with_key(&blob, &key, &salt).unwrap();
        let (out, _, _) = decrypt(&bytes, master).unwrap();
        assert_eq!(
            out.sessions_with_passwords
                .get("Proxmox Hosts/Elitedesk One"),
            Some(&"s3cret".to_string())
        );
    }

    #[test]
    fn wrong_password_fails() {
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_key(b"right", &salt).unwrap();
        let bytes = encrypt_with_key(&blob_with("x"), &key, &salt).unwrap();
        assert!(decrypt(&bytes, b"wrong").is_err());
    }

    #[test]
    fn rejects_non_vault_data() {
        assert!(decrypt(b"garbage", b"pw").is_err());
    }


    #[test]
    fn api_key_wrapped_vault_key_round_trips() {
        let key = "nxk1_test-key-for-unit-test-only";
        let vault_key = [7u8; KEY_LEN];
        let mut kek = kek_from_api_key(key);
        let sealed = seal_with_key(&kek, &vault_key).unwrap();
        kek.zeroize();

        let mut wrong_kek = kek_from_api_key("nxk1_a-different-key");
        let wrong = open_with_key(&wrong_kek, &sealed.0, &sealed.1);
        wrong_kek.zeroize();
        assert!(wrong.is_err());

        let mut right_kek = kek_from_api_key(key);
        let opened = open_with_key(&right_kek, &sealed.0, &sealed.1).unwrap();
        right_kek.zeroize();
        assert_eq!(opened, vault_key);
    }
}
