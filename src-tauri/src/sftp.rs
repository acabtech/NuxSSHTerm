//! SFTP commander backend — a persistent, scripted `sftp` child per tab.
//!
//! Strategy (verified against OpenSSH sftp 9.x on 2026-10-05):
//!
//! - One `sftp` process per commander tab, spawned WITHOUT `-b`. With piped
//!   stdin/stdout/stderr it stays alive indefinitely and processes one command
//!   per line, **echoing each command as `sftp> <cmd>` on stdout**.
//! - We frame every request with a sentinel: after the real command we push a
//!   `!printf 'NXSFTPEND_<session-token>\n'` local-shell command. The response
//!   for the request is everything on stdout up to and including the sentinel
//!   line, and everything on stderr that arrived in the same window (errors
//!   like `Can't ls: ...` go to stderr with CRLF endings).
//! - Errors do **not** abort the stream (verified: a failing `ls` is followed
//!   by a successful `pwd`), so a persistent child is safe for sequential ops.
//! - `sftp` has no `-r` for `rm` (OpenSSH rejects `rm -r`), so recursive
//!   delete is a depth-first walk over the same child (`rm` files, `rmdir`
//!   dirs). It also has no `-p`/`-l` port/user (those are sftp flags for
//!   preserve-perms and bandwidth) — see `LaunchSpec::sftp_args()`.
//! - Password auth uses the system ssh `SSH_ASKPASS` hook: a 0600 file in the
//!   config dir holds the vault password for the lifetime of the session and a
//!   tiny helper script cats it when ssh asks (Phase 1 vault dependency).

use crate::pty::AskPassFile;
use crate::ssh::LaunchSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::State;

/// A blocking line buffer shared between a reader thread and `exec`.
#[derive(Default)]
struct LineBuf {
    lines: Mutex<std::collections::VecDeque<String>>,
    cv: Condvar,
}

impl LineBuf {
    fn len(&self) -> usize {
        self.lines.lock().unwrap().len()
    }
    fn clear(&self) {
        self.lines.lock().unwrap().clear();
    }
    fn push(&self, line: String) {
        let mut l = self.lines.lock().unwrap();
        l.push_back(line);
        self.cv.notify_all();
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(reader: R, buf: Arc<LineBuf>) {
    std::thread::spawn(move || {
        let mut r = std::io::BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let t = line.strip_suffix('\n').unwrap_or(&line);
                    let t = t.strip_suffix('\r').unwrap_or(t);
                    buf.push(t.to_string());
                }
            }
        }
    });
}

/// One persistent sftp child + its framing state.
pub struct SftpSession {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    out: Arc<LineBuf>,
    err: Arc<LineBuf>,
    /// Serialises command windows (one request in flight at a time).
    lock: Mutex<()>,
    /// Unique sentinel printed as a bare line to stdout marking the end of a response.
    marker: String,
    askpass_file: Option<PathBuf>,
    /// Cleared when a timeout or I/O error leaves the command stream untrustworthy.
    healthy: AtomicBool,
}

/// Raw stdout/stderr of one sftp command window. `err` is empty on success.
#[derive(Debug, Clone, Serialize)]
pub struct ExecOutput {
    pub out: Vec<String>,
    pub err: Vec<String>,
}

const UNHEALTHY_SESSION: &str =
    "sftp session is no longer usable after a timeout, close, or I/O error; reopen it";

fn marker_cmd(marker: &str) -> String {
    format!("!printf '{marker}\\n'")
}

impl SftpSession {
    /// Kill the child, purge any late output, and permanently reject further
    /// commands. Once a command window has been abandoned its marker may arrive
    /// late, so reusing the stream would desynchronise responses (issues #47, #52).
    fn fail(&self, message: String) -> String {
        self.kill();
        self.out.clear();
        self.err.clear();
        message
    }

    fn exec(&self, cmd: &str, timeout: Duration) -> Result<ExecOutput, String> {
        let _guard = self.lock.lock().unwrap();
        if !self.healthy.load(Ordering::Acquire) {
            return Err(UNHEALTHY_SESSION.into());
        }
        let out_start = self.out.len();
        let err_start = self.err.len();
        {
            let mut w = self.stdin.lock().unwrap();
            w.write_all(cmd.as_bytes())
                .map_err(|e| self.fail(format!("sftp write failed: {e}")))?;
            w.write_all(b"\n")
                .map_err(|e| self.fail(format!("sftp write failed: {e}")))?;
            let mc = marker_cmd(&self.marker);
            w.write_all(mc.as_bytes())
                .map_err(|e| self.fail(format!("sftp write failed: {e}")))?;
            w.write_all(b"\n")
                .map_err(|e| self.fail(format!("sftp write failed: {e}")))?;
            w.flush()
                .map_err(|e| self.fail(format!("sftp flush failed: {e}")))?;
        }

        let marker = self.marker.clone();
        let deadline = Instant::now() + timeout;
        loop {
            // `kill()` wakes this condvar so a concurrent close/error is not
            // masked by a full command timeout (issue #57).
            if !self.healthy.load(Ordering::Acquire) {
                return Err(UNHEALTHY_SESSION.into());
            }
            let found = {
                let l = self.out.lines.lock().unwrap();
                l.iter().position(|x| x == &marker)
            };
            if let Some(idx) = found {
                let mut l = self.out.lines.lock().unwrap();
                let out: Vec<String> = l
                    .drain(..=idx)
                    .skip(out_start)
                    .filter(|s| s != &marker && !s.starts_with("sftp> "))
                    .collect();
                let mut e = self.err.lines.lock().unwrap();
                let err: Vec<String> = e.drain(err_start..).collect();
                return Ok(ExecOutput { out, err });
            }
            if Instant::now() >= deadline {
                return Err(self.fail(format!(
                    "sftp command timed out after {}s: {cmd}",
                    timeout.as_secs()
                )));
            }
            let l = self.out.lines.lock().unwrap();
            let _ = self
                .out
                .cv
                .wait_timeout(l, Duration::from_millis(200))
                .unwrap();
        }
    }

    /// Drain any initial banner ("Connected to host.", "Attached to …", CRLF
    /// noise) so subsequent windows are clean. Called right after spawn.
    fn drain_banner(&self) {
        self.out.clear();
        self.err.clear();
    }

    /// Kill and reap the child, marking the session unusable. Reaping here
    /// prevents zombies from every close/drop path (issue #56); waking waiters
    /// lets an in-flight `exec()` fail promptly instead of sleeping out its
    /// whole timeout (issue #57).
    fn kill(&self) {
        self.healthy.store(false, Ordering::Release);
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            if let Err(e) = c.wait() {
                crate::log::error("sftp_reap", &e.to_string());
            }
        }
        self.out.cv.notify_all();
    }
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        // Replacement under the same id and error paths can drop a session
        // without an explicit close: never leak its authenticated child (#56).
        self.kill();
        if let Some(f) = &self.askpass_file {
            let _ = std::fs::remove_file(f);
        }
    }
}

#[derive(Default)]
pub struct SftpState(pub Mutex<HashMap<String, Arc<SftpSession>>>);

/// Quote a path for the sftp command lexer (double quotes + backslash escapes;
/// no shell is involved on the receiving end). Control characters are rejected:
/// `exec()` writes one command per line, so a newline in a "quoted" path could
/// inject another command (issue #45).
pub fn quote(path: &str) -> Result<String, String> {
    if path.chars().any(char::is_control) {
        return Err(format!(
            "invalid path containing a control character: {path:?}"
        ));
    }
    Ok(format!(
        "\"{}\"",
        path.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// `chmod` accepts an octal mode token (`755`, `0644`, `4755`, …). Anything
/// else — especially whitespace — must never reach the command stream (issues #46, #49).
fn validate_mode(mode: &str) -> Result<&str, String> {
    if (1..=4).contains(&mode.len()) && mode.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        Ok(mode)
    } else {
        Err(format!(
            "invalid chmod mode {mode:?}: expected 1-4 octal digits"
        ))
    }
}

/// Validate the `uid[:gid]` token used by `chown` before interpolation (issues #46, #49).
fn validate_uid(uid: &str) -> Result<&str, String> {
    let all_digits =
        |s: &str| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit());
    let valid = uid
        .split_once(':')
        .is_some_and(|(u, g)| all_digits(u) && all_digits(g))
        || all_digits(uid);
    if valid {
        Ok(uid)
    } else {
        Err(format!(
            "invalid chown uid {uid:?}: expected digits or uid:gid"
        ))
    }
}

/// A parsed `ls -la` line. Columns: `perms nlink owner group size mon day
/// time|year name...` (name = tokens ≥9 joined with spaces).
///
/// OpenSSH sftp's `ls` prints **full paths** (verified: `ls -la /tmp` yields
/// `/tmp/foo`), so `path` is the full path ready for ops and `name` is the
/// basename for display (`.` and `..` keep their short forms).
#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    /// Basename for display (`.`, `..`, or the file/dir name).
    pub name: String,
    /// Full path, suitable for `sftp_op` targets.
    pub path: String,
    pub perms: String,
    pub nlink: String,
    pub owner: String,
    pub group: String,
    pub size: u64,
    /// Raw server-side date string, e.g. "Oct  5 12:37" or "Dec 24  2023".
    pub mtime: String,
    pub is_dir: bool,
    pub is_link: bool,
}

/// Last path segment (`/tmp/a/b` → `b`, `/tmp/a/.` → `.`, `/` → ``).
pub fn base_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) if i + 1 < path.len() => &path[i + 1..],
        Some(_) => "",
        None => path,
    }
}

pub fn parse_ls_line(line: &str) -> Option<SftpEntry> {
    let tok: Vec<&str> = line.split_whitespace().collect();
    if tok.len() < 9 {
        return None; // "total N" headers, blank lines, noise
    }
    let size = tok[4].parse::<u64>().ok()?;
    let perms = tok[0];
    let mut path = tok[8..].join(" ");
    // For symlinks `ls -la` prints `link -> target`. The ` -> target` suffix is
    // display-only: every later op (rm/get/rm_r/F9) must use the link path
    // itself, so strip it once here and derive the display name from that.
    if perms.starts_with('l')
        && let Some((link, _)) = path.split_once(" -> ")
    {
        path = link.to_string();
    }
    let name = base_name(&path).to_string();
    Some(SftpEntry {
        name,
        path,
        perms: perms.to_string(),
        nlink: tok[1].to_string(),
        owner: tok[2].to_string(),
        group: tok[3].to_string(),
        size,
        mtime: format!("{} {} {}", tok[5], tok[6], tok[7]),
        is_dir: perms.starts_with('d'),
        is_link: perms.starts_with('l'),
    })
}

/// Typed remote operations, dispatched by `sftp_op` with correct quoting.
/// `get`/`put` carry a longer default timeout because transfers can be slow.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SftpOp {
    Mkdir {
        path: String,
    },
    Rmdir {
        path: String,
    },
    Rm {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Chmod {
        mode: String,
        path: String,
    },
    Chown {
        uid: String,
        path: String,
    },
    Symlink {
        target: String,
        link: String,
    },
    Get {
        remote: String,
        local: String,
    },
    Put {
        local: String,
        remote: String,
    },
    /// Recursive delete via depth-first walk (sftp has no `rm -r`).
    RmR {
        path: String,
        is_dir: bool,
    },
}

/// Depth-first recursive delete on the *same* sftp child (no extra auth, no
/// shell). Symlinks are removed as files (the link itself), never followed.
fn rm_r(
    sess: &SftpSession,
    path: &str,
    is_dir: bool,
    warns: &mut Vec<String>,
) -> Result<(), String> {
    if !is_dir {
        let r = sess.exec(&format!("rm {}", quote(path)?), Duration::from_secs(20))?;
        if !r.err.is_empty() {
            warns.push(format!("{path}: {}", r.err.join("; ")));
        }
        return Ok(());
    }
    let r = sess.exec(&format!("ls -la {}", quote(path)?), Duration::from_secs(20))?;
    if !r.err.is_empty() {
        warns.push(format!("{path}: {}", r.err.join("; ")));
        return Ok(());
    }
    let kids: Vec<SftpEntry> = r
        .out
        .iter()
        .filter_map(|l| parse_ls_line(l))
        .filter(|e| {
            let b = base_name(&e.name);
            b != "." && b != ".."
        })
        .collect();
    for k in kids {
        // k.path is the full path — sftp `ls` prints full paths.
        rm_r(sess, &k.path, k.is_dir, warns)?;
    }
    let r = sess.exec(&format!("rmdir {}", quote(path)?), Duration::from_secs(20))?;
    if !r.err.is_empty() {
        warns.push(format!("{path}: {}", r.err.join("; ")));
    }
    Ok(())
}

fn askpass_setup(id: &str, password: &str) -> Result<(PathBuf, PathBuf), String> {
    let dir = crate::store::config_dir();
    let helper = dir.join("sftp-askpass.sh");
    if !helper.exists() {
        let _ = std::fs::write(&helper, "#!/bin/sh\ncat \"${NX_SFTP_ASKPASS_FILE}\"\n");
        let _ =
            std::fs::set_permissions(&helper, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    let file = dir.join(format!(".sftp-askpass-{}", sanitize_id(id)));
    crate::pty::write_secret_file(&file, password)?;
    Ok((helper, file))
}

/// Askpass ids come from user-visible names; keep only filename-safe chars.
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Spawn the persistent child for one commander tab, run a `pwd` handshake to
/// drain the banner and learn the remote home, and return it + remote cwd.
pub fn open(
    id: &str,
    spec: &LaunchSpec,
    password: Option<String>,
) -> Result<(SftpSession, String), String> {
    let auth = crate::store::get_ui_settings().auth;
    // agent_only promises "only ssh-agent identities (ignore stored
    // credential)", so drop the stored credential before computing policy
    // or building argv (issue #21).
    let mut effective_spec = spec.clone();
    let askpass_password: Option<String> = if auth.suppresses_credentials() {
        effective_spec.private_key = String::new();
        None
    } else {
        password.clone().filter(|p| !p.is_empty())
    };
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
            "sftp_open",
            "agent forwarding requested but no usable agent is available for export; forwarding will offer no keys",
        );
    }

    effective_spec.forward_agent = spec.forward_agent && can_export_agent;

    let mut cmd = Command::new("sftp");
    // sftp_args() ends with the host target, so the auth policy options
    // (IdentitiesOnly pinning) must go before it.
    let mut argv = effective_spec.sftp_args();
    let host = argv.pop().unwrap_or_default();
    if identities_only {
        argv.push("-o".into());
        argv.push("IdentitiesOnly=yes".into());
    }
    argv.push(host);
    for a in argv {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SSH auth ordering (issue #8): export the key-manager agent socket so the
    // sftp child can use agent keys too (and ForwardAgent when the spec asks
    // for it), unless the policy pins the session credential agent-free.
    if let Some(sock) = agent_socket {
        cmd.env("SSH_AUTH_SOCK", &sock);
    }

    // The guard removes the file if spawn/pipe setup fails before the session
    // owns it (issue #51).
    let mut askpass_file = AskPassFile(None);
    if let Some(pw) = askpass_password {
        let (helper, file) = askpass_setup(id, &pw)?;
        cmd.env("SSH_ASKPASS", &helper);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        // Any non-empty DISPLAY satisfies ssh's askpass engagement check.
        cmd.env("DISPLAY", ":0");
        cmd.env("NX_SFTP_ASKPASS_FILE", &file);
        askpass_file = AskPassFile(Some(file));
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn sftp: {e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "sftp stdin unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sftp stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "sftp stderr unavailable".to_string())?;

    // All early-return paths above are past: the session now owns removing the
    // askpass file on drop.
    let askpass_file = askpass_file.release();

    let out = Arc::new(LineBuf::default());
    let err = Arc::new(LineBuf::default());
    spawn_reader(stdout, out.clone());
    spawn_reader(stderr, err.clone());

    let marker = format!("NXSFTPEND_{:08x}", rand::random::<u32>());
    let sess = SftpSession {
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        out: out.clone(),
        err: err.clone(),
        lock: Mutex::new(()),
        marker,
        askpass_file,
        healthy: AtomicBool::new(true),
    };

    let hs = sess.exec("pwd", Duration::from_secs(20));
    match hs {
        Ok(resp) => {
            let cwd = resp
                .out
                .iter()
                .find_map(|l| l.strip_prefix("Remote working directory: "))
                .map(str::to_string);
            sess.drain_banner();
            let Some(cwd) = cwd else {
                sess.kill();
                let detail = if resp.err.is_empty() {
                    "no 'pwd' response from sftp (check host/user/key)".to_string()
                } else {
                    resp.err.join(" | ")
                };
                return Err(detail);
            };
            Ok((sess, cwd))
        }
        Err(e) => {
            sess.kill();
            Err(e)
        }
    }
}

/* ------------------------------- commands ------------------------------- */

#[tauri::command]
pub fn sftp_open(
    state: State<'_, SftpState>,
    id: String,
    spec: LaunchSpec,
    password: Option<String>,
) -> Result<String, String> {
    let (sess, cwd) = open(&id, &spec, password)?;
    // Arc lets commands hold a session without holding the map mutex, so one
    // slow transfer cannot block every other tab (issue #57).
    state.0.lock().unwrap().insert(id, Arc::new(sess));
    Ok(cwd)
}

#[tauri::command]
pub fn sftp_close(state: State<'_, SftpState>, id: String) -> Result<(), String> {
    // Release the map lock before killing/reaping: an in-flight `exec()` may
    // still hold an Arc, but no other tab is blocked (issues #56, #57).
    let removed = state.0.lock().unwrap().remove(&id);
    if let Some(s) = removed {
        s.kill();
    }
    Ok(())
}

#[tauri::command]
pub fn sftp_list(
    state: State<'_, SftpState>,
    id: String,
    path: String,
) -> Result<Vec<SftpEntry>, String> {
    // Clone the Arc and drop the map lock before the blocking `exec()`
    // (issue #57).
    let sess = state
        .0
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("unknown sftp session")?;
    let r = sess.exec(
        &format!("ls -la {}", quote(&path)?),
        Duration::from_secs(20),
    )?;
    if !r.err.is_empty() {
        return Err(r.err.join(" | "));
    }
    Ok(r.out.iter().filter_map(|l| parse_ls_line(l)).collect())
}

/// Dispatch a typed remote op. Returns the raw window so the UI can surface
/// transfer messages; an error is returned when stderr carried failure text.
#[tauri::command]
pub fn sftp_op(
    state: State<'_, SftpState>,
    id: String,
    op: SftpOp,
    timeout_secs: Option<u64>,
) -> Result<ExecOutput, String> {
    // Same as `sftp_list`: never hold the global map lock across `exec()`
    // (issue #57).
    let sess = state
        .0
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("unknown sftp session")?;
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(20).max(10));
    let cmd = match &op {
        SftpOp::Mkdir { path } => format!("mkdir {}", quote(path)?),
        SftpOp::Rmdir { path } => format!("rmdir {}", quote(path)?),
        SftpOp::Rm { path } => format!("rm {}", quote(path)?),
        SftpOp::Rename { from, to } => format!("rename {} {}", quote(from)?, quote(to)?),
        SftpOp::Chmod { mode, path } => {
            format!("chmod {} {}", validate_mode(mode)?, quote(path)?)
        }
        SftpOp::Chown { uid, path } => {
            format!("chown {} {}", validate_uid(uid)?, quote(path)?)
        }
        SftpOp::Symlink { target, link } => {
            format!("symlink {} {}", quote(target)?, quote(link)?)
        }
        SftpOp::Get { remote, local } => {
            format!("get -r {} {}", quote(remote)?, quote(local)?)
        }
        SftpOp::Put { local, remote } => {
            format!("put -r {} {}", quote(local)?, quote(remote)?)
        }
        SftpOp::RmR { path, is_dir, .. } => {
            let mut warns = Vec::new();
            rm_r(sess.as_ref(), path, *is_dir, &mut warns)?;
            return if warns.is_empty() {
                Ok(ExecOutput {
                    out: Vec::new(),
                    err: Vec::new(),
                })
            } else {
                Err(warns.join("\n"))
            };
        }
    };
    let r = sess.exec(&cmd, timeout)?;
    if !r.err.is_empty() {
        return Err(r.err.join(" | "));
    }
    Ok(r)
}

/* --------------------------------- tests --------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(line: &str) -> SftpEntry {
        parse_ls_line(line).unwrap_or_else(|| panic!("failed to parse: {line:?}"))
    }

    #[test]
    fn parses_regular_file() {
        let e = entry("-rw-r--r--   1 sam  sam     1234 Jan  5 10:30 file.txt");
        assert_eq!(e.name, "file.txt");
        assert_eq!(e.perms, "-rw-r--r--");
        assert_eq!(e.owner, "sam");
        assert_eq!(e.group, "sam");
        assert_eq!(e.size, 1234);
        assert_eq!(e.mtime, "Jan 5 10:30");
        assert!(!e.is_dir);
        assert!(!e.is_link);
    }

    #[test]
    fn parses_directory_and_link() {
        let d = entry("drwxr-xr-x   2 sam  sam     4096 Dec 24  2023 some dir");
        assert!(d.is_dir);
        assert_eq!(d.name, "some dir");
        let l = entry("lrwxrwxrwx   1 sam  sam       19 Jan  5 10:30 link -> /target");
        assert!(l.is_link);
        assert!(!l.is_dir);
        assert_eq!(l.name, "link");
        assert_eq!(l.path, "link");
    }

    #[test]
    fn strips_symlink_target_from_full_path() {
        // Regression: OpenSSH sftp prints full paths for symlinks, and the
        // ` -> target` suffix must never leak into the op path.
        let e = entry("lrwxrwxrwx   1 u g 10 Oct  5 12:34 /tmp/dir/link -> /tmp/dir/target");
        assert!(e.is_link);
        assert_eq!(e.path, "/tmp/dir/link");
        assert_eq!(e.name, "link");
    }

    #[test]
    fn handles_question_mark_nlink_from_sftp_server() {
        // OpenSSH sftp-server emits '?' for nlink in `ls -la` output.
        let e = entry("drwxrwxrwt   ? root     root         1500 Oct  5 12:34 /tmp/.");
        assert_eq!(e.nlink, "?");
        assert_eq!(e.owner, "root");
        assert_eq!(e.size, 1500);
        assert_eq!(e.name, ".");
        assert_eq!(e.path, "/tmp/.");
    }

    #[test]
    fn names_are_basenames_paths_are_full() {
        // sftp `ls` prints full paths; the parser splits display name + op path.
        let e = entry("-rwxr-xr-x   ? sam      sam            90 Oct  4 08:10 /tmp/esp32url.txt");
        assert_eq!(e.name, "esp32url.txt");
        assert_eq!(e.path, "/tmp/esp32url.txt");
        assert!(e.perms.starts_with('-'));
    }

    #[test]
    fn skips_total_and_short_lines() {
        assert!(parse_ls_line("total 512").is_none());
        assert!(parse_ls_line("").is_none());
        assert!(parse_ls_line("noise").is_none());
        assert!(parse_ls_line("drwxr-xr-x   2 sam  sam     4096 Dec 24").is_none());
    }

    #[test]
    fn quote_escapes_double_quotes_and_backslashes() {
        assert_eq!(quote("plain").unwrap(), "\"plain\"");
        assert_eq!(quote("a b").unwrap(), "\"a b\"");
        assert_eq!(quote("say \"hi\"").unwrap(), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn quote_rejects_control_characters() {
        for path in ["a\nb", "a\rb", "a\0b", "a\u{1b}b"] {
            assert!(
                quote(path).is_err(),
                "accepted control character in {path:?}"
            );
        }
    }

    #[test]
    fn validates_mode_and_uid_before_command_building() {
        assert_eq!(validate_mode("755").unwrap(), "755");
        assert_eq!(validate_mode("0644").unwrap(), "0644");
        assert_eq!(validate_mode("4755").unwrap(), "4755");
        for mode in ["", "8", "u+x", "755\nchown 0 /tmp", "12345"] {
            assert!(validate_mode(mode).is_err(), "accepted mode {mode:?}");
        }

        assert_eq!(validate_uid("0").unwrap(), "0");
        assert_eq!(validate_uid("1000").unwrap(), "1000");
        assert_eq!(validate_uid("1000:1000").unwrap(), "1000:1000");
        for uid in ["", "root", "-1\nrm /tmp", "1000:", ":1000", "99999999999"] {
            assert!(validate_uid(uid).is_err(), "accepted uid {uid:?}");
        }
    }

    #[test]
    fn timeout_marks_session_unhealthy() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sleep child");
        let stdin = child.stdin.take().unwrap();
        let out = Arc::new(LineBuf::default());
        let err = Arc::new(LineBuf::default());
        spawn_reader(child.stdout.take().unwrap(), out.clone());
        spawn_reader(child.stderr.take().unwrap(), err.clone());
        let sess = SftpSession {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            out,
            err,
            lock: Mutex::new(()),
            marker: "NXSFTPEND_test".into(),
            askpass_file: None,
            healthy: AtomicBool::new(true),
        };

        let message = sess.fail("timed out".into());
        assert_eq!(message, "timed out");
        assert!(!sess.healthy.load(Ordering::Acquire));
        let err = sess.exec("pwd", Duration::from_millis(10)).unwrap_err();
        assert!(err.contains("reopen it"), "unexpected error: {err}");
        let mut child = sess.child.lock().unwrap();
        assert!(
            child.try_wait().unwrap().is_some(),
            "failed child was not reaped"
        );
        let _ = child.wait();
    }

    #[test]
    fn marker_cmd_is_single_quoted() {
        let m = marker_cmd("NXSFTPEND_0000abcd");
        assert_eq!(m, "!printf 'NXSFTPEND_0000abcd\\n'");
    }

    /// End-to-end driver smoke test against the throwaway `nux-sftp-test`
    /// local user (real sshd on localhost), covering both password (askpass)
    /// and key auth plus the full op battery. Run explicitly with:
    ///   cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
    /// Skipped (not failed) when the test user is absent.
    #[test]
    #[ignore = "requires local nux-sftp-test user + sshd on localhost"]
    fn smoke_localhost_driver() {
        use std::process::Command as Cmd;
        let id_ok = Cmd::new("id")
            .arg("nux-sftp-test")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !id_ok {
            eprintln!("skipping: nux-sftp-test user not present");
            return;
        }

        let mut spec = LaunchSpec {
            host: "localhost".into(),
            port: 22,
            username: "nux-sftp-test".into(),
            ..Default::default()
        };

        // --- password/askpass path ---
        let (sess, cwd) =
            open("smoke-pass", &spec, Some("NuxSftpTest!2026".into())).expect("password auth open");
        assert_eq!(cwd, "/home/nux-sftp-test");
        drive_ops(&sess);
        sess.kill();

        // --- key-auth path ---
        spec.private_key = "/home/user/.ssh/nux-sftp-test_ed25519".into();
        let (sess2, _) = open("smoke-key", &spec, None).expect("key auth open");
        drive_ops(&sess2);
        sess2.kill();
    }

    fn drive_ops(sess: &SftpSession) {
        const D: Duration = Duration::from_secs(20);
        let r = sess.exec("mkdir \"/tmp/nx-smoke\"", D).expect("mkdir");
        assert!(r.err.is_empty(), "mkdir errors: {:?}", r.err);
        let r = sess
            .exec("put -r \"/etc/hostname\" \"/tmp/nx-smoke/hostname.txt\"", D)
            .expect("put");
        assert!(r.err.is_empty(), "put errors: {:?}", r.err);
        let r = sess.exec("ls -la \"/tmp/nx-smoke\"", D).expect("ls");
        assert!(r.err.is_empty());
        let entries: Vec<SftpEntry> = r.out.iter().filter_map(|l| parse_ls_line(l)).collect();
        assert!(entries.iter().any(|e| e.name == "hostname.txt"
            && e.path == "/tmp/nx-smoke/hostname.txt"
            && e.size > 0));
        let r = sess
            .exec(
                "rename \"/tmp/nx-smoke/hostname.txt\" \"/tmp/nx-smoke/renamed.txt\"",
                D,
            )
            .expect("rename");
        assert!(r.err.is_empty());
        let r = sess
            .exec("chmod 700 \"/tmp/nx-smoke/renamed.txt\"", D)
            .expect("chmod");
        assert!(r.err.is_empty());
        let r = sess
            .exec("chown 0 \"/tmp/nx-smoke/renamed.txt\"", D)
            .expect("chown numeric");
        assert!(!r.err.is_empty(), "non-root chown should fail");
        let r = sess
            .exec(
                "get -r \"/tmp/nx-smoke/renamed.txt\" \"/tmp/nx-smoke-fetched.txt\"",
                D,
            )
            .expect("get");
        assert!(r.err.is_empty());
        assert!(
            std::fs::read_to_string("/tmp/nx-smoke-fetched.txt")
                .expect("fetched file")
                .trim()
                .len()
                > 0
        );
        let r = sess
            .exec(
                "symlink \"/tmp/nx-smoke/renamed.txt\" \"/tmp/nx-smoke/link\"",
                D,
            )
            .expect("symlink");
        assert!(r.err.is_empty());
        // recursive delete walk (dir with a file + a symlink)
        let mut warns = Vec::new();
        rm_r(sess, "/tmp/nx-smoke", true, &mut warns).expect("rm_r");
        assert!(warns.is_empty(), "rm_r warnings: {warns:?}");
        assert!(!std::path::Path::new("/tmp/nx-smoke").exists());
        let _ = std::fs::remove_file("/tmp/nx-smoke-fetched.txt");
    }
}
