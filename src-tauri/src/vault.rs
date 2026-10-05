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
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;
use zeroize::Zeroize;

const MAGIC: &[u8; 8] = b"NXVAULT1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

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

/// Decrypt an on-disk layout back into the blob, the derived key, and its salt.
fn decrypt(data: &[u8], master: &[u8]) -> Result<(VaultBlob, [u8; KEY_LEN], [u8; SALT_LEN]), String> {
    let header = MAGIC.len() + SALT_LEN + NONCE_LEN;
    if data.len() < header || &data[..MAGIC.len()] != MAGIC {
        return Err("not a NuxSSHTerm vault file".into());
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&data[MAGIC.len()..MAGIC.len() + SALT_LEN]);
    let nonce = &data[MAGIC.len() + SALT_LEN..header];
    let ct = &data[header..];

    let key = derive_key(master, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "wrong master password or corrupted vault".to_string())?;
    let blob = serde_json::from_slice(&pt).map_err(|e| e.to_string())?;
    Ok((blob, key, salt))
}

/// Persist the current in-memory blob to disk using the retained key + salt.
fn save(state: &VaultState) -> Result<(), String> {
    let inner = state.0.lock().unwrap();
    let key = inner.key.ok_or("vault is locked")?;
    let salt = inner.salt.ok_or("vault is locked")?;
    let bytes = encrypt_with_key(&inner.blob, &key, &salt)?;
    std::fs::write(vault_path(), bytes).map_err(|e| e.to_string())
}

/// Load + decrypt the vault into memory, marking it unlocked.
fn load(state: &VaultState, master: &[u8]) -> Result<(), String> {
    let data = std::fs::read(vault_path()).map_err(|e| e.to_string())?;
    let (blob, key, salt) = decrypt(&data, master)?;
    let mut inner = state.0.lock().unwrap();
    inner.blob = blob;
    inner.key = Some(key);
    inner.salt = Some(salt);
    Ok(())
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
    std::fs::write(vault_path(), bytes).map_err(|e| e.to_string())?;
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

/// Lock the vault, dropping the in-memory key, salt, and blob.
#[tauri::command]
pub fn vault_lock(state: State<'_, VaultState>) -> Result<(), String> {
    let mut inner = state.0.lock().unwrap();
    inner.blob = VaultBlob::default();
    if let Some(mut k) = inner.key.take() {
        k.zeroize();
    }
    inner.salt = None;
    Ok(())
}

/// Forgot password: wipe the vault (passwords lost, connections kept).
#[tauri::command]
pub fn vault_reset(state: State<'_, VaultState>) -> Result<(), String> {
    let _ = std::fs::remove_file(vault_path());
    let mut inner = state.0.lock().unwrap();
    inner.blob = VaultBlob::default();
    if let Some(mut k) = inner.key.take() {
        k.zeroize();
    }
    inner.salt = None;
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
            out.sessions_with_passwords.get("Proxmox Hosts/Elitedesk One"),
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
}