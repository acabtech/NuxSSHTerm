//! WinSSHTerm password decryption.
//!
//! WinSSHTerm never stores connection passwords in plaintext. A backup contains:
//!
//!   * `<Node ... Password="<base64 AES-256-CBC ciphertext>">`
//!   * a `key` file (base64 protobuf field 3 of the backup) holding two random
//!     keys, itself encrypted with the WinSSHTerm master password.
//!
//! The recovery chain implemented here mirrors `WinSSHTerm.Tools.AesCrypt` and
//! `WinSSHTerm.Core.Runtime.LoadKeyFileV2`:
//!
//!   1. key-file password = `DEFAULT_KEY_FILE_PASSPHRASE` + user master password
//!      (the user part is empty when no master password was set)
//!   2. decrypt `key[1..]` with `AesCrypt`'s master-password scheme → base64 of
//!      64 interleaved key bytes
//!   3. de-interleave with bitwise NOT → `PasswordKey` / `SaltKey`
//!   4. verify by decrypting `VerifyKey` → `KEY_FILE_SEC_STRING`
//!   5. decrypt each `Password` with `PasswordKey` / `SaltKey`
//!
//! Both key derivations use PBKDF2-HMAC-SHA1 with 1012 iterations. The AES key
//! is 32 bytes, the IV the next 16 bytes of the same PBKDF2 stream. Passwords
//! are encrypted as `UTF-8(password + 14-character pepper)`.

use aes::Aes256;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;

/// Default passphrase WinSSHTerm uses to protect the key file before a custom
/// master password is set. When a master password *is* set, the effective
/// passphrase is this string followed by the master password.
pub const DEFAULT_KEY_FILE_PASSPHRASE: &str = "h7ko%.rdz.WFxsS218LK";

/// `General.KeyFileSecString`: decrypted `VerifyKey` must equal this value.
pub const KEY_FILE_SEC_STRING: &str = "47f58e2a5e2418bef1865a858be8f5ef";

/// 14-character marker appended before encryption (16 UTF-8 bytes).
const PEPPER: [u8; 16] = [
    116, 53, 55, 105, 46, 33, 103, 100, 57, 195, 182, 195, 159, 102, 116, 121,
];

/// Salt used by the master-password key derivation.
const MP_SALT: [u8; 16] = [
    59, 218, 49, 183, 72, 5, 80, 227, 188, 102, 4, 109, 239, 201, 81, 168,
];

const ITERATIONS: u32 = 1012;
const KEY_LEN: usize = 32;
const IV_LEN: usize = 16;
const PEPPER_CHARS: usize = 14;

type Aes256CbcDec = cbc::Decryptor<Aes256>;

/// Derive the AES key and IV from a PBKDF2 password/salt pair.
fn derive(password: &[u8], salt: &[u8]) -> ([u8; KEY_LEN], [u8; IV_LEN]) {
    let mut stream = [0u8; KEY_LEN + IV_LEN];
    pbkdf2_hmac::<Sha1>(password, salt, ITERATIONS, &mut stream);
    let mut key = [0u8; KEY_LEN];
    let mut iv = [0u8; IV_LEN];
    key.copy_from_slice(&stream[..KEY_LEN]);
    iv.copy_from_slice(&stream[KEY_LEN..]);
    (key, iv)
}

fn aes_decrypt(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, String> {
    let decryptor = Aes256CbcDec::new_from_slices(key, iv)
        .map_err(|_| "invalid WinSSHTerm AES key/IV".to_string())?;
    decryptor
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| "wrong WinSSHTerm master password or corrupted data".to_string())
}

/// Remove the 14-character pepper WinSSHTerm appends to every plaintext.
fn strip_pepper(plain: &str) -> Result<String, String> {
    let count = plain.chars().count();
    if count < PEPPER_CHARS {
        return Err("decrypted WinSSHTerm value is too short".into());
    }
    Ok(plain.chars().take(count - PEPPER_CHARS).collect())
}

/// Decrypt a value protected with the master-password branch (`EncryptWithMP`).
fn decrypt_with_password(cipher_b64: &str, passphrase: &str) -> Result<String, String> {
    let ciphertext = B64
        .decode(cipher_b64.as_bytes())
        .map_err(|e| format!("invalid WinSSHTerm ciphertext: {e}"))?;

    let mut password = passphrase.as_bytes().to_vec();
    password.extend_from_slice(&PEPPER);
    let (key, iv) = derive(&password, &MP_SALT);

    let plain = aes_decrypt(&ciphertext, &key, &iv)?;
    let text = String::from_utf8(plain)
        .map_err(|_| "decrypted WinSSHTerm value is not valid UTF-8".to_string())?;
    strip_pepper(&text)
}

/// Decrypt a connection password with the key-file `PasswordKey`/`SaltKey`.
pub fn decrypt_value(cipher_b64: &str, password_key: &[u8], salt_key: &[u8]) -> Result<String, String> {
    let ciphertext = B64
        .decode(cipher_b64.as_bytes())
        .map_err(|e| format!("invalid WinSSHTerm ciphertext: {e}"))?;
    let (key, iv) = derive(password_key, salt_key);
    let plain = aes_decrypt(&ciphertext, &key, &iv)?;
    let text = String::from_utf8(plain)
        .map_err(|_| "decrypted WinSSHTerm value is not valid UTF-8".to_string())?;
    strip_pepper(&text)
}

/// Build the passphrase used to unlock the WinSSHTerm `key` file.
fn key_file_passphrase(master_password: Option<&str>) -> String {
    let mut pass = String::from(DEFAULT_KEY_FILE_PASSPHRASE);
    if let Some(mp) = master_password {
        pass.push_str(mp);
    }
    pass
}

/// Recover `(PasswordKey, SaltKey)` from a WinSSHTerm `key` file.
///
/// The file layout is `[0x02][AES ciphertext]`; the ciphertext decrypts to the
/// base64 of 64 interleaved key bytes. A wrong master password fails the AES
/// padding check.
pub fn decrypt_key_file(
    key_file: &[u8],
    master_password: Option<&str>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if key_file.len() < 2 {
        return Err("WinSSHTerm key file is empty or truncated".into());
    }
    if key_file[0] != 2 {
        return Err(format!(
            "unsupported WinSSHTerm key file version {}",
            key_file[0]
        ));
    }

    let passphrase = key_file_passphrase(master_password);
    let cipher_b64 = B64.encode(&key_file[1..]);
    let inner_b64 = decrypt_with_password(&cipher_b64, &passphrase)?;

    let interleaved = B64
        .decode(inner_b64.as_bytes())
        .map_err(|e| format!("WinSSHTerm key file payload is not valid base64: {e}"))?;
    if interleaved.len() != 64 {
        return Err(format!(
            "WinSSHTerm key file payload has {} bytes, expected 64",
            interleaved.len()
        ));
    }

    let password_key: Vec<u8> = (0..32).map(|i| !interleaved[2 * i]).collect();
    let salt_key: Vec<u8> = (0..32).map(|i| !interleaved[2 * i + 1]).collect();
    Ok((password_key, salt_key))
}

/// Verify a recovered key pair against the backup's `VerifyKey`.
///
/// Returns `Ok(())` when `VerifyKey` decrypts to `KEY_FILE_SEC_STRING`, i.e. the
/// key file was unlocked with the correct WinSSHTerm master password.
pub fn verify_key_pair(
    password_key: &[u8],
    salt_key: &[u8],
    verify_key_b64: &str,
) -> Result<(), String> {
    let value = decrypt_value(verify_key_b64, password_key, salt_key)?;
    if value == KEY_FILE_SEC_STRING {
        Ok(())
    } else {
        Err("WinSSHTerm VerifyKey does not match — wrong master password".into())
    }
}

/// Heuristic used for plain `connections.xml` exports (no embedded key file):
/// WinSSHTerm ciphertexts are base64 of a non-UTF-8, block-aligned AES payload.
pub fn looks_encrypted(value: &str) -> bool {
    let Ok(bytes) = B64.decode(value.as_bytes()) else {
        return false;
    };
    if bytes.len() < 16 || bytes.len() % 16 != 0 {
        return false;
    }
    std::str::from_utf8(&bytes).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};

    type Aes256CbcEnc = cbc::Encryptor<Aes256>;

    fn encrypt_with_password(plain: &str, passphrase: &str) -> String {
        let mut password = passphrase.as_bytes().to_vec();
        password.extend_from_slice(&PEPPER);
        let (key, iv) = derive(&password, &MP_SALT);
        let mut buf = Vec::new();
        buf.extend_from_slice(plain.as_bytes());
        buf.extend_from_slice(&PEPPER);
        let enc = Aes256CbcEnc::new_from_slices(&key, &iv).unwrap();
        B64.encode(enc.encrypt_padded_vec_mut::<Pkcs7>(&buf))
    }

    fn encrypt_with_keys(plain: &str, password_key: &[u8], salt_key: &[u8]) -> String {
        let (key, iv) = derive(password_key, salt_key);
        let mut buf = Vec::new();
        buf.extend_from_slice(plain.as_bytes());
        buf.extend_from_slice(&PEPPER);
        let enc = Aes256CbcEnc::new_from_slices(&key, &iv).unwrap();
        B64.encode(enc.encrypt_padded_vec_mut::<Pkcs7>(&buf))
    }

    /// Build a WinSSHTerm `key` file for a synthetic key pair.
    fn make_key_file(
        password_key: &[u8],
        salt_key: &[u8],
        master_password: Option<&str>,
    ) -> Vec<u8> {
        let mut interleaved = Vec::with_capacity(64);
        for i in 0..32 {
            interleaved.push(!password_key[i]);
            interleaved.push(!salt_key[i]);
        }
        let inner = B64.encode(&interleaved);
        let cipher = encrypt_with_password(&inner, &key_file_passphrase(master_password));
        let mut file = vec![2u8];
        file.extend_from_slice(&B64.decode(cipher).unwrap());
        file
    }

    #[test]
    fn round_trips_key_file_and_password() {
        let pk: Vec<u8> = (0..32).map(|i| i as u8 + 1).collect();
        let sk: Vec<u8> = (0..32).map(|i| i as u8 + 101).collect();
        let key_file = make_key_file(&pk, &sk, Some("master-pw"));

        let (got_pk, got_sk) = decrypt_key_file(&key_file, Some("master-pw")).unwrap();
        assert_eq!(got_pk, pk);
        assert_eq!(got_sk, sk);

        let verify = encrypt_with_keys(KEY_FILE_SEC_STRING, &pk, &sk);
        verify_key_pair(&pk, &sk, &verify).unwrap();

        let cipher = encrypt_with_keys("s3cret!", &pk, &sk);
        assert_eq!(decrypt_value(&cipher, &pk, &sk).unwrap(), "s3cret!");
    }

    #[test]
    fn key_file_without_master_password_uses_default() {
        let pk: Vec<u8> = (0..32).map(|i| 200 - i as u8).collect();
        let sk: Vec<u8> = (0..32).map(|i| 50 + i as u8).collect();
        let key_file = make_key_file(&pk, &sk, None);
        let (got_pk, got_sk) = decrypt_key_file(&key_file, None).unwrap();
        assert_eq!(got_pk, pk);
        assert_eq!(got_sk, sk);
    }

    #[test]
    fn wrong_master_password_is_rejected() {
        let pk = vec![7u8; 32];
        let sk = vec![9u8; 32];
        let key_file = make_key_file(&pk, &sk, Some("right"));
        assert!(decrypt_key_file(&key_file, Some("wrong")).is_err());
        assert!(decrypt_key_file(&key_file, None).is_err());
    }

    #[test]
    fn verify_key_detects_wrong_key_pair() {
        let pk = vec![1u8; 32];
        let sk = vec![2u8; 32];
        let verify = encrypt_with_keys(KEY_FILE_SEC_STRING, &pk, &sk);
        assert!(verify_key_pair(&pk, &sk, &verify).is_ok());
        assert!(verify_key_pair(&vec![3u8; 32], &sk, &verify).is_err());
    }

    /// Real-file end-to-end check. Set both environment variables:
    ///   WINSSHTERM_BACKUP=~/Downloads/WinSSHTerm.xml
    ///   WINSSHTERM_MASTER_PASSWORD=...   (omit when no master password is set)
    #[test]
    fn decrypts_real_backup_from_env() {
        let Ok(path) = std::env::var("WINSSHTERM_BACKUP") else {
            return;
        };
        let master = std::env::var("WINSSHTERM_MASTER_PASSWORD").ok();
        let bytes = std::fs::read(path).expect("read WINSSHTERM_BACKUP");
        let text = String::from_utf8_lossy(&bytes);
        let backup = crate::xml::parse_backup_envelope(&text).expect("parse backup");
        let key_file = backup.key_file.expect("backup should embed a key file");
        let verify = backup.verify_key.expect("backup should embed a VerifyKey");

        let (pk, sk) = decrypt_key_file(&key_file, master.as_deref())
            .expect("key file should decrypt with the supplied master password");
        verify_key_pair(&pk, &sk, &verify).expect("VerifyKey should match");

        let mut decrypted = 0usize;
        let mut flat: Vec<(Vec<String>, crate::model::Node)> = Vec::new();
        crate::model::flatten(&backup.nodes, &[], &mut flat);
        for (_, node) in flat {
            if node.node_type == crate::model::KIND_CONNECTION && !node.password.is_empty() {
                let plain = decrypt_value(&node.password, &pk, &sk)
                    .expect("every real password should decrypt");
                assert!(!plain.is_empty());
                assert_ne!(plain, node.password, "plaintext must not equal ciphertext");
                decrypted += 1;
            }
        }
        assert!(decrypted > 0, "expected encrypted passwords in the real backup");
    }
}
