//! PTY-backed terminal sessions.
//!
//! A PTY is opened and `ssh` is spawned inside it. Output is streamed to the
//! frontend as `pty-data` events; input/resize/kill arrive as commands.

use crate::ssh::LaunchSpec;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

static NEXT_SESSION_TOKEN: AtomicU64 = AtomicU64::new(1);

pub struct Sess {
    /// Distinguishes successive sessions reusing the same tab id, so a stale
    /// reader cannot remove its replacement (issue #55).
    token: u64,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    /// Temporary 0600 file holding the vault password for `SSH_ASKPASS`.
    askpass_file: Option<PathBuf>,
}

impl Drop for Sess {
    fn drop(&mut self) {
        // Every removal path (explicit close, replacement under the same id,
        // reader-thread cleanup on child exit) must reap the child and delete
        // the plaintext askpass file (issues #55, #56).
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            if let Err(e) = child.wait() {
                crate::log::error("pty_reap", &e.to_string());
            }
        }
        if let Some(f) = &self.askpass_file {
            let _ = std::fs::remove_file(f);
        }
    }
}

#[derive(Default)]
pub struct PtyState(pub Mutex<HashMap<String, Sess>>);

#[derive(Clone, serde::Serialize)]
pub struct DataEvent {
    pub id: String,
    /// base64-encoded raw PTY bytes — binary-safe across 8 KiB chunk boundaries.
    pub data: String,
}

#[derive(Clone, serde::Serialize)]
pub struct ExitEvent {
    pub id: String,
    pub status: String,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Create the `SSH_ASKPASS` helper + per-session password file used to answer
/// ssh's password prompt from a vault password.
///
/// Mirrors the SFTP askpass mechanism: a tiny 0700 helper cats a 0600 file
/// holding the password; `SSH_ASKPASS_REQUIRE=force` makes ssh use it even
/// though the session has a controlling PTY (it would otherwise prompt on the
/// terminal and ignore askpass).
fn askpass_setup(id: &str, password: &str) -> Result<(PathBuf, PathBuf), String> {
    let dir = crate::store::config_dir();
    let helper = dir.join("pty-askpass.sh");
    if !helper.exists() {
        let _ = std::fs::write(&helper, "#!/bin/sh\ncat \"${NX_PTY_ASKPASS_FILE}\"\n");
        let _ =
            std::fs::set_permissions(&helper, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    let safe_id: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = dir.join(format!(".pty-askpass-{safe_id}"));
    write_secret_file(&file, password)?;
    Ok((helper, file))
}

/// Create `path` containing `contents` with mode 0600 from the instant it is
/// created (never briefly umask-readable), and propagate write failures.
/// A stale file from a killed session is removed first so `create_new` cannot
/// spuriously fail on the next open (issue #39).
pub(crate) fn write_secret_file(path: &std::path::Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|e| format!("failed to create secret file {}: {e}", path.display()))?;
    f.write_all(contents.as_bytes())
        .map_err(|e| format!("failed to write secret file {}: {e}", path.display()))
}

/// RAII guard for a temporary askpass password file: removes the file on drop
/// unless `release()` transfers ownership into a long-lived session (which
/// removes it in its own `Drop`). Covers open error paths that return before
/// the session is constructed (issues #50, #51).
pub(crate) struct AskPassFile(pub(crate) Option<PathBuf>);

impl Drop for AskPassFile {
    fn drop(&mut self) {
        if let Some(f) = &self.0 {
            let _ = std::fs::remove_file(f);
        }
    }
}

impl AskPassFile {
    /// Disarm the guard and hand the path to a session.
    pub(crate) fn release(mut self) -> Option<PathBuf> {
        self.0.take()
    }
}

/// Open a terminal: spawn `ssh` (or a local shell when host is empty) inside a PTY.
#[tauri::command]
pub fn pty_open(
    app: AppHandle,
    state: State<'_, PtyState>,
    id: String,
    spec: LaunchSpec,
    cols: u16,
    rows: u16,
    password: Option<String>,
) -> Result<(), String> {
    let pair = native_pty_system().openpty(size(cols, rows)).map_err(|e| {
        let msg = format!("openpty failed: {e}");
        crate::log::error("pty_open", &msg);
        msg
    })?;

    let local_shell = spec.host.is_empty();
    // SSH auth ordering (issue #8): decide whether the agent socket is exported
    // and whether the configured credential is pinned with `IdentitiesOnly`.
    let mut auth_args: Vec<String> = Vec::new();
    let mut auth_env: Vec<(String, String)> = Vec::new();
    let mut effective_spec = spec.clone();
    let mut askpass_password: Option<String> = None;
    if !local_shell {
        let auth = crate::store::get_ui_settings().auth;
        // agent_only promises "only ssh-agent identities (ignore stored
        // credential)", so drop the stored credential before computing policy
        // or building argv (issue #21).
        if auth.suppresses_credentials() {
            effective_spec.private_key = String::new();
        } else {
            askpass_password = password.clone().filter(|p| !p.is_empty());
        }
        let has_credential = !effective_spec.private_key.is_empty()
            || askpass_password.as_deref().is_some_and(|p| !p.is_empty());
        let (policy_export_agent, identities_only) = auth.session_policy(has_credential);
        let agent_socket = if policy_export_agent {
            crate::agent::session_socket()
        } else {
            None
        };
        let can_export_agent = agent_socket.is_some();
        if spec.forward_agent && !can_export_agent {
            crate::log::error(
                "pty_open",
                "agent forwarding requested but no usable agent is available for export; forwarding will offer no keys",
            );
        }
        if identities_only {
            auth_args.push("-o".into());
            auth_args.push("IdentitiesOnly=yes".into());
        }
        if let Some(sock) = agent_socket {
            auth_env.push(("SSH_AUTH_SOCK".to_string(), sock));
        }
        effective_spec.forward_agent = spec.forward_agent && can_export_agent;
    }
    let mut cmd = if local_shell {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        CommandBuilder::new(shell)
    } else {
        let mut c = CommandBuilder::new("ssh");
        // ssh_args() ends with the host target, so the auth policy options
        // (IdentitiesOnly pinning) must go before it.
        let mut argv = effective_spec.ssh_args();
        let host = argv.pop().unwrap_or_default();
        argv.extend(auth_args);
        argv.push(host);
        for a in argv {
            c.arg(a);
        }
        for (k, v) in auth_env {
            c.env(k, v);
        }
        c
    };
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if !spec.username.is_empty() {
        cmd.env("USER", spec.username.clone());
    }
    // Feed a vault password to ssh's password prompt via askpass. This is what
    // makes saved passwords work for terminal sessions (sftp already does the
    // same in `sftp::open`); without it ssh always prompts on the PTY.
    // Suppressed for `agent_only` (askpass_password stays None above).
    // The guard removes the file if any later step fails before the session
    // owns it (issue #50).
    let mut askpass_file = AskPassFile(None);
    if !local_shell && let Some(pw) = askpass_password {
        let (helper, file) = askpass_setup(&id, &pw)?;
        cmd.env("SSH_ASKPASS", &helper);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        // Any non-empty DISPLAY satisfies ssh's askpass engagement check.
        cmd.env("DISPLAY", ":0");
        cmd.env("NX_PTY_ASKPASS_FILE", &file);
        askpass_file = AskPassFile(Some(file));
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| {
        let msg = format!("failed to spawn session: {e}");
        crate::log::error("pty_open", &msg);
        msg
    })?;
    drop(pair.slave); // so the child sees EOF when it exits

    let mut reader = pair.master.try_clone_reader().map_err(|e| {
        let msg = format!("pty reader failed: {e}");
        crate::log::error("pty_open", &msg);
        msg
    })?;
    let writer = pair.master.take_writer().map_err(|e| {
        let msg = format!("pty writer failed: {e}");
        crate::log::error("pty_open", &msg);
        msg
    })?;
    // All early-return paths above are past: the session now owns removing the
    // askpass file on drop.
    let askpass_file = askpass_file.release();

    let token = NEXT_SESSION_TOKEN.fetch_add(1, Ordering::Relaxed);
    let sess = Sess {
        token,
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child: Mutex::new(child),
        askpass_file,
    };
    // Insert before starting the reader: a fast child exit must still find the
    // session so its askpass file is cleaned up (issue #55).
    state.0.lock().unwrap().insert(id.clone(), sess);

    // Stream PTY output to the frontend.
    let app_for_thread = app.clone();
    let id_for_thread = id;
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let data = B64.encode(&buf[..n]);
                    let _ = app_for_thread.emit(
                        "pty-data",
                        DataEvent {
                            id: id_for_thread.clone(),
                            data,
                        },
                    );
                }
            }
        }
        let _ = app_for_thread.emit(
            "pty-exit",
            ExitEvent {
                id: id_for_thread.clone(),
                status: "closed".into(),
            },
        );

        // The ssh process is gone. Drop the Sess now so its askpass file is
        // deleted and the child reaped, without waiting for tab unmount (#55).
        if let Some(state) = app_for_thread.try_state::<PtyState>() {
            let mut map = state.0.lock().unwrap();
            // Only remove our own generation: a fast reopen under the same id
            // must not be torn down by this old reader (#55).
            if map.get(&id_for_thread).is_some_and(|s| s.token == token) {
                map.remove(&id_for_thread);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn pty_write(state: State<'_, PtyState>, id: String, data: String) -> Result<(), String> {
    let map = state.0.lock().unwrap();
    let sess = map.get(&id).ok_or("unknown session")?;
    let mut w = sess.writer.lock().unwrap();
    w.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, PtyState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let map = state.0.lock().unwrap();
    let sess = map.get(&id).ok_or("unknown session")?;
    sess.master
        .lock()
        .unwrap()
        .resize(size(cols, rows))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_close(state: State<'_, PtyState>, id: String) -> Result<(), String> {
    // `Sess::drop` kills, reaps, and removes the askpass file (issue #56).
    let _ = state.0.lock().unwrap().remove(&id);
    Ok(())
}

#[tauri::command]
pub fn pty_alive(state: State<'_, PtyState>) -> Vec<String> {
    state.0.lock().unwrap().keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::{AskPassFile, write_secret_file};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn secret_file_is_0600_and_replaces_stale_files() {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "nx-secret-file-test-{}-{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);

        write_secret_file(&path, "first").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        // A leftover file from a killed session must not break the next open.
        write_secret_file(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn askpass_guard_removes_file_on_drop() {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "nx-askpass-guard-test-{}-{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        write_secret_file(&path, "secret").unwrap();
        {
            let _guard = AskPassFile(Some(path.clone()));
            assert!(path.exists());
        }
        assert!(!path.exists(), "askpass guard left the file behind");
    }
}
