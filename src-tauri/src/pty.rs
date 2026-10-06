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
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

pub struct Sess {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
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

/// Open a terminal: spawn `ssh` (or a local shell when host is empty) inside a PTY.
#[tauri::command]
pub fn pty_open(
    app: AppHandle,
    state: State<'_, PtyState>,
    id: String,
    spec: LaunchSpec,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let pair = native_pty_system()
        .openpty(size(cols, rows))
        .map_err(|e| {
            let msg = format!("openpty failed: {e}");
            crate::log::error("pty_open", &msg);
            msg
        })?;

    let local_shell = spec.host.is_empty();
    let mut cmd = if local_shell {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        CommandBuilder::new(shell)
    } else {
        let mut c = CommandBuilder::new("ssh");
        for a in spec.ssh_args() {
            c.arg(a);
        }
        c
    };
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if !spec.username.is_empty() {
        cmd.env("USER", spec.username.clone());
    }
    // Export the key-manager agent socket (Phase 4) so terminal sessions can
    // use agent keys and ForwardAgent even when launched from a desktop icon.
    if let Some(sock) = crate::agent::configured_socket() {
        cmd.env("SSH_AUTH_SOCK", &sock);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| {
            let msg = format!("failed to spawn session: {e}");
            crate::log::error("pty_open", &msg);
            msg
        })?;
    drop(pair.slave); // so the child sees EOF when it exits

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| {
            let msg = format!("pty reader failed: {e}");
            crate::log::error("pty_open", &msg);
            msg
        })?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| {
            let msg = format!("pty writer failed: {e}");
            crate::log::error("pty_open", &msg);
            msg
        })?;

    // Stream PTY output to the frontend.
    let app_for_thread = app.clone();
    let id_for_thread = id.clone();
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
    });

    state.0.lock().unwrap().insert(
        id,
        Sess {
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
        },
    );
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
    let mut map = state.0.lock().unwrap();
    if let Some(sess) = map.remove(&id)
        && let Ok(mut child) = sess.child.lock() {
            let _ = child.kill();
        }
    Ok(())
}

#[tauri::command]
pub fn pty_alive(state: State<'_, PtyState>) -> Vec<String> {
    state.0.lock().unwrap().keys().cloned().collect()
}