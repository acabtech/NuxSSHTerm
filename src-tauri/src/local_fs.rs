//! Local pane filesystem ops for the SFTP commander (tokio fs).
//!
//! Kept deliberately small: list / mkdir / rmdir / rm / rename / remove (the
//! last is the local half of an F6 move — dir → remove_dir_all, symlink or
//! file → remove_file, never following symlinks).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LocalEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    /// Local time "YYYY-MM-DD HH:MM" (computed in-process, no chrono dep).
    pub mtime: String,
    /// POSIX-style mode string, e.g. "-rw-r--r--".
    pub perms: String,
}

pub fn perms_str(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let mut out = String::with_capacity(10);
    out.push(if meta.file_type().is_symlink() {
        'l'
    } else if meta.is_dir() {
        'd'
    } else {
        '-'
    });
    let bits = [0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001];
    let chars = ['r', 'w', 'x', 'r', 'w', 'x', 'r', 'w', 'x'];
    for (b, c) in bits.iter().zip(chars.iter()) {
        out.push(if mode & b != 0 { *c } else { '-' });
    }
    out
}

/// Format a mtime as local "YYYY-MM-DD HH:MM".
/// (Civil-date conversion via the Howard Hinnant `civil_from_days` algorithm —
/// no chrono/time dependency. Local-time *zone* offset is not applied; UTC is
/// close enough for a file manager and documented in ARCHITECTURE.md.)
pub fn fmt_mtime(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m) = (rem / 3600, (rem % 3600) / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}")
}

/// Local user home — the commander's default local pane directory.
#[tauri::command]
pub fn home_dir() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub async fn local_list(path: String) -> Result<Vec<LocalEntry>, String> {
    let mut entries = Vec::new();
    let mut rd = tokio::fs::read_dir(&path)
        .await
        .map_err(|e| format!("{path}: {e}"))?;
    while let Some(ent) = rd.next_entry().await.map_err(|e| e.to_string())? {
        let name = ent.file_name().to_string_lossy().to_string();
        let ft = ent.file_type().await.ok();
        let is_link = ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
        let is_dir = ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
        let meta = tokio::fs::symlink_metadata(ent.path()).await.ok();
        entries.push(LocalEntry {
            name,
            is_dir,
            is_link,
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            mtime: meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(fmt_mtime)
                .unwrap_or_default(),
            perms: meta.as_ref().map(perms_str).unwrap_or_default(),
        });
    }
    Ok(entries)
}

#[tauri::command]
pub async fn local_mkdir(path: String) -> Result<(), String> {
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("{path}: {e}"))
}

#[tauri::command]
pub async fn local_rmdir(path: String) -> Result<(), String> {
    tokio::fs::remove_dir(&path)
        .await
        .map_err(|e| format!("{path}: {e}"))
}

#[tauri::command]
pub async fn local_rm(path: String) -> Result<(), String> {
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("{path}: {e}"))
}

#[tauri::command]
pub async fn local_rename(from: String, to: String) -> Result<(), String> {
    tokio::fs::rename(&from, &to)
        .await
        .map_err(|e| format!("{from} → {to}: {e}"))
}

/// Delete a local entry without ever following symlinks.
#[tauri::command]
pub async fn local_remove(path: String, is_dir: bool) -> Result<(), String> {
    let meta = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|e| format!("{path}: {e}"))?;
    if meta.file_type().is_symlink() {
        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| format!("{path}: {e}"))
    } else if meta.is_dir() {
        if is_dir {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|e| format!("{path}: {e}"))
        } else {
            tokio::fs::remove_dir(&path)
                .await
                .map_err(|e| format!("{path}: {e}"))
        }
    } else {
        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| format!("{path}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_mtime_known_epoch() {
        // 2026-10-05 00:00:00 UTC
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_791_158_400);
        assert_eq!(fmt_mtime(t), "2026-10-05 00:00");
    }

    #[test]
    fn epoch_zero_and_boundaries() {
        let t = std::time::UNIX_EPOCH;
        assert_eq!(fmt_mtime(t), "1970-01-01 00:00");
        // 2024-02-29 23:59 (leap day)
        let t2 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_709_251_140);
        assert_eq!(fmt_mtime(t2), "2024-02-29 23:59");
    }

    #[test]
    fn perms_strings() {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata("/bin/sh").expect("shell present");
        let s = perms_str(&meta);
        assert!(s.starts_with('-') || s.starts_with('l') || s.starts_with('d'));
        assert_eq!(s.len(), 10);
        let _ = PermissionsExt::mode(&meta.permissions()); // compile-check import use
    }
}