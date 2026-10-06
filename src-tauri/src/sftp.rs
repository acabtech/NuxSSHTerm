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

use crate::ssh::LaunchSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
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
}

/// Raw stdout/stderr of one sftp command window. `err` is empty on success.
#[derive(Debug, Clone, Serialize)]
pub struct ExecOutput {
    pub out: Vec<String>,
    pub err: Vec<String>,
}

fn marker_cmd(marker: &str) -> String {
    format!("!printf '{marker}\\n'")
}

impl SftpSession {
    fn exec(&self, cmd: &str, timeout: Duration) -> Result<ExecOutput, String> {
        let _guard = self.lock.lock().unwrap();
        let out_start = self.out.len();
        let err_start = self.err.len();
        {
            let mut w = self.stdin.lock().unwrap();
            w.write_all(cmd.as_bytes())
                .map_err(|e| format!("sftp write failed: {e}"))?;
            w.write_all(b"\n")
                .map_err(|e| format!("sftp write failed: {e}"))?;
            let mc = marker_cmd(&self.marker);
            w.write_all(mc.as_bytes())
                .map_err(|e| format!("sftp write failed: {e}"))?;
            w.write_all(b"\n")
                .map_err(|e| format!("sftp write failed: {e}"))?;
            w.flush().map_err(|e| format!("sftp flush failed: {e}"))?;
        }

        let marker = self.marker.clone();
        let deadline = Instant::now() + timeout;
        loop {
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
                self.out.clear();
                self.err.clear();
                return Err(format!(
                    "sftp command timed out after {}s: {cmd}",
                    timeout.as_secs()
                ));
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

    fn kill(&self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        if let Some(f) = &self.askpass_file {
            let _ = std::fs::remove_file(f);
        }
    }
}

#[derive(Default)]
pub struct SftpState(pub Mutex<HashMap<String, SftpSession>>);

/// Quote a path for the sftp command lexer (double quotes + backslash escapes;
/// no shell is involved on the receiving end).
pub fn quote(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
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
    let path = tok[8..].join(" ");
    // For symlinks ls prints `link -> target`; keep the link's own name for
    // display (the full text remains `path` for ops / F9 properties).
    let name = if perms.starts_with('l') {
        match path.split_once(" -> ") {
            Some((link, _)) => link.to_string(),
            None => base_name(&path).to_string(),
        }
    } else {
        base_name(&path).to_string()
    };
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
    Mkdir { path: String },
    Rmdir { path: String },
    Rm { path: String },
    Rename { from: String, to: String },
    Chmod { mode: String, path: String },
    Chown { uid: String, path: String },
    Symlink { target: String, link: String },
    Get { remote: String, local: String },
    Put { local: String, remote: String },
    /// Recursive delete via depth-first walk (sftp has no `rm -r`).
    RmR { path: String, is_dir: bool },
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
        let r = sess.exec(&format!("rm {}", quote(path)), Duration::from_secs(20))?;
        if !r.err.is_empty() {
            warns.push(format!("{path}: {}", r.err.join("; ")));
        }
        return Ok(());
    }
    let r = sess.exec(&format!("ls -la {}", quote(path)), Duration::from_secs(20))?;
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
    let r = sess.exec(&format!("rmdir {}", quote(path)), Duration::from_secs(20))?;
    if !r.err.is_empty() {
        warns.push(format!("{path}: {}", r.err.join("; ")));
    }
    Ok(())
}

fn askpass_setup(id: &str, password: &str) -> (PathBuf, PathBuf) {
    let dir = crate::store::config_dir();
    let helper = dir.join("sftp-askpass.sh");
    if !helper.exists() {
        let _ = std::fs::write(
            &helper,
            "#!/bin/sh\ncat \"${NX_SFTP_ASKPASS_FILE}\"\n",
        );
        let _ = std::fs::set_permissions(&helper, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    let file = dir.join(format!(".sftp-askpass-{id}"));
    let _ = std::fs::write(&file, password);
    let _ = std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    (helper, file)
}

/// Spawn the persistent child for one commander tab, run a `pwd` handshake to
/// drain the banner and learn the remote home, and return it + remote cwd.
pub fn open(
    id: &str,
    spec: &LaunchSpec,
    password: Option<String>,
) -> Result<(SftpSession, String), String> {
    let mut cmd = Command::new("sftp");
    for a in spec.sftp_args() {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Export the key-manager agent socket (Phase 4) so the sftp child can use
    // agent keys too (and ForwardAgent when the spec asks for it).
    if let Some(sock) = crate::agent::configured_socket() {
        cmd.env("SSH_AUTH_SOCK", &sock);
    }

    let mut askpass_file = None;
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        let (helper, file) = askpass_setup(id, &pw);
        cmd.env("SSH_ASKPASS", &helper);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        // Any non-empty DISPLAY satisfies ssh's askpass engagement check.
        cmd.env("DISPLAY", ":0");
        cmd.env("NX_SFTP_ASKPASS_FILE", &file);
        askpass_file = Some(file);
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

    let out = Arc::new(LineBuf::default());
    let err = Arc::new(LineBuf::default());
    spawn_reader(stdout, out.clone());
    spawn_reader(stderr, err.clone());

    let marker = format!(
        "NXSFTPEND_{:08x}",
        rand::random::<u32>()
    );
    let sess = SftpSession {
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        out: out.clone(),
        err: err.clone(),
        lock: Mutex::new(()),
        marker,
        askpass_file,
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
    state.0.lock().unwrap().insert(id, sess);
    Ok(cwd)
}

#[tauri::command]
pub fn sftp_close(state: State<'_, SftpState>, id: String) -> Result<(), String> {
    let mut map = state.0.lock().unwrap();
    if let Some(s) = map.remove(&id) {
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
    let map = state.0.lock().unwrap();
    let sess = map.get(&id).ok_or("unknown sftp session")?;
    let r = sess.exec(&format!("ls -la {}", quote(&path)), Duration::from_secs(20))?;
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
    let map = state.0.lock().unwrap();
    let sess = map.get(&id).ok_or("unknown sftp session")?;
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(20).max(10));
    let cmd = match &op {
        SftpOp::Mkdir { path } => format!("mkdir {}", quote(path)),
        SftpOp::Rmdir { path } => format!("rmdir {}", quote(path)),
        SftpOp::Rm { path } => format!("rm {}", quote(path)),
        SftpOp::Rename { from, to } => format!("rename {} {}", quote(from), quote(to)),
        SftpOp::Chmod { mode, path } => format!("chmod {mode} {}", quote(path)),
        SftpOp::Chown { uid, path } => format!("chown {uid} {}", quote(path)),
        SftpOp::Symlink { target, link } => format!("symlink {} {}", quote(target), quote(link)),
        SftpOp::Get { remote, local } => format!("get -r {} {}", quote(remote), quote(local)),
        SftpOp::Put { local, remote } => format!("put -r {} {}", quote(local), quote(remote)),
        SftpOp::RmR { path, is_dir, .. } => {
            let mut warns = Vec::new();
            rm_r(sess, path, *is_dir, &mut warns)?;
            return if warns.is_empty() {
                Ok(ExecOutput { out: Vec::new(), err: Vec::new() })
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
        assert_eq!(l.path, "link -> /target");
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
        let e = entry(
            "-rwxr-xr-x   ? sam      sam            90 Oct  4 08:10 /tmp/esp32url.txt",
        );
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
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a b"), "\"a b\"");
        assert_eq!(quote("say \"hi\""), "\"say \\\"hi\\\"\"");
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
        let (sess, cwd) = open("smoke-pass", &spec, Some("NuxSftpTest!2026".into()))
            .expect("password auth open");
        assert_eq!(cwd, "/home/nux-sftp-test");
        drive_ops(&sess);
        sess.kill();

        // --- key-auth path ---
        spec.private_key = "/home/sam/.ssh/nux-sftp-test_ed25519".into();
        let (sess2, _) = open("smoke-key", &spec, None).expect("key auth open");
        drive_ops(&sess2);
        sess2.kill();
    }

    fn drive_ops(sess: &SftpSession) {
        const D: Duration = Duration::from_secs(20);
        let r = sess
            .exec("mkdir \"/tmp/nx-smoke\"", D)
            .expect("mkdir");
        assert!(r.err.is_empty(), "mkdir errors: {:?}", r.err);
        let r = sess
            .exec("put -r \"/etc/hostname\" \"/tmp/nx-smoke/hostname.txt\"", D)
            .expect("put");
        assert!(r.err.is_empty(), "put errors: {:?}", r.err);
        let r = sess
            .exec("ls -la \"/tmp/nx-smoke\"", D)
            .expect("ls");
        assert!(r.err.is_empty());
        let entries: Vec<SftpEntry> = r.out.iter().filter_map(|l| parse_ls_line(l)).collect();
        assert!(entries
            .iter()
            .any(|e| e.name == "hostname.txt" && e.path == "/tmp/nx-smoke/hostname.txt" && e.size > 0));
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
            .exec("get -r \"/tmp/nx-smoke/renamed.txt\" \"/tmp/nx-smoke-fetched.txt\"", D)
            .expect("get");
        assert!(r.err.is_empty());
        assert!(std::fs::read_to_string("/tmp/nx-smoke-fetched.txt")
            .expect("fetched file")
            .trim()
            .len()
            > 0);
        let r = sess
            .exec("symlink \"/tmp/nx-smoke/renamed.txt\" \"/tmp/nx-smoke/link\"", D)
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