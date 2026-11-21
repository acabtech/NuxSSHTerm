//! API key generation and management (issue #25).
//!
//! Provides scoped bearer credentials for the CLI/automation surface so
//! scripts and LLM agents do not need to interact with the vault master
//! password. Security model:
//!
//! - Keys are 32 random bytes, base64url-encoded, prefixed `nxk1_`.
//! - Only a SHA-256 hash of the key (plus a random salt, like the agent
//!   record, purely for readability) is stored in `settings.json` under
//!   `apiKeys`; the plaintext is shown exactly once at creation.
//! - Keys are verified with a constant-time comparison of hashes.
//! - Keys are bearer credentials for the opt-in automation surface: a
//!   registered key can wrap the vault key for unattended unlock (issue #22),
//!   and revoking it invalidates both CLI use and that wrapped copy.
//! - The CLI never prints stored hashes; `apikeys list` shows name, id,
//!   creation date, and last-used (reserved) metadata only.

use crate::store::ApiKeyRecord;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Prefix of every generated key (visible without any secret knowledge).
pub const KEY_PREFIX: &str = "nxk1_";

/// A newly minted API key: the full plaintext (shown once) and its record.
pub struct GeneratedKey {
    /// Full key string `nxk1_<43 base64url chars>` — must be shown/saved now.
    pub plaintext: String,
    pub record: ApiKeyRecord,
}

/// SHA-256 hex digest of a key plaintext.
pub fn hash_key(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// True when `candidate` matches the stored hash for `record`.
pub fn verify(record: &ApiKeyRecord, candidate: &str) -> bool {
    let stored = record.key_hash.as_bytes();
    let candidate_hash = hash_key(candidate);
    let supplied = candidate_hash.as_bytes();
    if stored.len() != supplied.len() {
        return false;
    }
    stored.ct_eq(supplied).into()
}

/// Generate a new key with the given label. The plaintext is returned once
/// and never persisted; only its hash goes into `settings.json`.
pub fn generate(name: &str) -> Result<GeneratedKey, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err("key name must be 1–64 characters".into());
    }
    if crate::store::api_key_name_taken(name) {
        return Err(format!("an API key named {name:?} already exists"));
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let plaintext = format!("{KEY_PREFIX}{}", base64_url(&bytes));
    Ok(GeneratedKey {
        record: hash_record(&plaintext, name),
        plaintext,
    })
}

fn hash_record(plaintext: &str, name: &str) -> ApiKeyRecord {
    let key_hash = hash_key(plaintext);
    // Key id: first 8 hex of the hash (stable, non-secret).
    ApiKeyRecord {
        id: key_hash[..8].to_string(),
        name: name.to_string(),
        key_hash,
        created: crate::store::now_iso(),
        last_used: String::new(),
    }
}

fn base64_url(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_match_prefix_and_hash() {
        let k = generate("ci").unwrap();
        assert!(k.plaintext.starts_with(KEY_PREFIX));
        assert_eq!(k.plaintext.len(), KEY_PREFIX.len() + 43);
        assert_eq!(k.record.key_hash, hash_key(&k.plaintext));
        assert_eq!(k.record.id.len(), 8);
        assert_eq!(k.record.name, "ci");
    }

    #[test]
    fn verify_accepts_correct_and_rejects_wrong() {
        let k = generate("t").unwrap();
        assert!(verify(&k.record, &k.plaintext));
        assert!(!verify(&k.record, "nxk1_wrong"));
    }

    #[test]
    fn empty_or_long_name_rejected() {
        assert!(generate("").is_err());
        assert!(generate(&"x".repeat(65)).is_err());
    }

    #[test]
    fn keys_are_unique() {
        let a = generate("a").unwrap();
        let b = generate("b").unwrap();
        assert_ne!(a.plaintext, b.plaintext);
        assert_ne!(a.record.key_hash, b.record.key_hash);
    }
}
