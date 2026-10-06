//! Key manager backend — the Pageant equivalent (Phase 4).
//!
//! Strategy (verified against OpenSSH 9.x on 2026-10-06):
//!
//! - If `SSH_AUTH_SOCK` is already set (a desktop keyring, an ssh-agent started
//!   by the shell, …) we adopt that agent: `present=true, ours=false`. We never
//!   touch agents we do not own.
//! - If unset we offer to spawn a **dedicated** `ssh-agent -a <sock>` in
//!   daemon mode. The launcher exits immediately after printing
//!   `SSH_AUTH_SOCK=…; SSH_AGENT_PID=…;` to stdout — the agent itself keeps
//!   running (Pageant-like: keys survive app restarts), so we persist
//!   `agent_socket` + `agent_pid` in `settings.json` and can re-attach to it
//!   next launch. `agent_stop` runs `ssh-agent -k` with both env vars set
//!   (that kills only the daemon on our socket) and clears the settings.
//! - All children (terminal PTYs and sftp drivers) get `SSH_AUTH_SOCK`
//!   exported by `agent::configured_socket()` so key auth + agent forwarding
//!   work even when the app was started from a desktop launcher (no env).
//! - `.ppk` keys are converted by the Phase 2 importer; this module deals only
//!   with OpenSSH key paths via `ssh-add`.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

/// One identity reported by `ssh-add -l`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentKey {
    /// Key size in bits (first whitespace token).
    pub bits: String,
    /// Fingerprint (second token; `SHA256:…` or hex).
    pub fingerprint: String,
    /// Human comment (middle tokens, may contain spaces).
    pub comment: String,
    /// `(ED25519)` / `(RSA)` style suffix, minus the parens, if present.
    pub key_type: String,
    /// The key path this identity was added from ("" when added by another tool).
    /// `ssh-add -d` needs a path, so per-key Remove is only enabled when known.
    pub source_path: String,
}

/// Session-scoped record of the key paths this app added (fingerprint → path),
/// so the panel can offer per-identity removal over the lifetime of the app.
#[derive(Default)]
pub struct AgentState(Mutex<AgentInner>);

#[derive(Default)]
struct AgentInner {
    /// fingerprint (peg to the `ssh-add -l` token) -> key file path
    added: HashMap<String, String>,
}

/// Status snapshot for the key-manager panel.
#[derive(Clone, Serialize)]
pub struct AgentStatus {
    /// True when a usable agent socket exists (ours or external).
    pub present: bool,
    /// True when `socket` is the dedicated agent this app spawned.
    pub ours: bool,
    /// The socket path in use (None when no agent is present).
    pub socket: Option<String>,
    pub keys: Vec<AgentKey>,
}

/// Result of an `ssh-add <key>` attempt, so the UI can prompt for a passphrase.
#[derive(Clone, Serialize)]
pub struct AddKeyResult {
    pub added: bool,
    /// True when the key is passphrase-protected and no (or a wrong) passphrase was tried.
    pub needs_passphrase: bool,
    pub message: String,
}

/* ------------------------- socket resolution ------------------------- */

fn env_socket() -> Option<String> {
    let v = std::env::var("SSH_AUTH_SOCK").unwrap_or_default();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// The persisted socket path, but only when the socket file actually exists.
fn persisted_socket_file() -> Option<String> {
    let (sock, _) = crate::store::agent_record();
    if sock.is_empty() || !PathBuf::from(&sock).exists() {
        return None;
    }
    Some(sock)
}

/// The effective socket we export to child sessions: the inherited
/// `SSH_AUTH_SOCK` when set, otherwise our persisted agent socket when its
/// file still exists. Never an error path — child spawns just skip the env var.
pub fn configured_socket() -> Option<String> {
    if let Some(s) = env_socket() {
        return Some(s);
    }
    persisted_socket_file()
}

/* ------------------------------ commands ------------------------------ */

/// Snapshot of the agent situation + its identities (`ssh-add -l`).
#[tauri::command]
pub fn agent_status(state: State<'_, AgentState>) -> AgentStatus {
    if let Some(sock) = configured_socket() {
        // Ours when we persist it ourselves (env sockets are never ours).
        let ours = env_socket().is_none();
        let (out, err, code) = run_ssh_add(vec!["-l".to_string()], sock.clone());
        let (alive, mut keys) = parse_add_l(&out, &err, code);
        let added = state.0.lock().unwrap().added.clone();
        for k in &mut keys {
            if let Some(p) = added.get(&k.fingerprint) {
                k.source_path = p.clone();
            }
        }
        AgentStatus {
            present: alive,
            ours: alive && ours,
            socket: if alive { Some(sock) } else { None },
            keys,
        }
    } else {
        AgentStatus {
            present: false,
            ours: false,
            socket: None,
            keys: Vec::new(),
        }
    }
}

/// Spawn a dedicated `ssh-agent` (only when no agent is usable) and adopt it.
#[tauri::command]
pub fn agent_start(state: State<'_, AgentState>) -> Result<AgentStatus, String> {
    if configured_socket().is_some() {
        return Ok(agent_status(state));
    }

    let dir = crate::store::config_dir();
    let sock = dir.join("agent.sock");
    let sock_str = sock.to_string_lossy().to_string();

    // A stale socket file from a dead agent would make ssh-agent refuse to bind.
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }

    let mut cmd = std::process::Command::new("ssh-agent");
    cmd.arg("-a").arg(&sock_str);
    let out = cmd.output().map_err(|e| format!("could not start ssh-agent: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        let err2 = String::from_utf8_lossy(&out.stdout).into_owned();
        return Err(format!("ssh-agent failed: {err}{err2}"));
    }

    let pid = parse_agent_pid(&String::from_utf8_lossy(&out.stdout).into_owned());
    if pid.is_empty() {
        return Err("ssh-agent started but its PID could not be parsed".into());
    }
    if !sock.exists() {
        return Err(format!("ssh-agent started but socket was not created: {sock_str}"));
    }

    crate::store::set_agent_record(&sock_str, &pid);
    Ok(agent_status(state))
}

/// Stop the dedicated agent (ours only) via `ssh-agent -k`.
#[tauri::command]
pub fn agent_stop() -> Result<(), String> {
    if env_socket().is_some() {
        return Err("SSH_AUTH_SOCK points to an external agent — not managed by NuxSSHTerm".into());
    }
    let (sock, pid) = crate::store::agent_record();
    if sock.is_empty() || !PathBuf::from(&sock).exists() {
        // Nothing running on a socket we track; just clear the record.
        crate::store::set_agent_record("", "");
        return Ok(());
    }
    let mut cmd = std::process::Command::new("ssh-agent");
    cmd.arg("-k");
    cmd.env("SSH_AUTH_SOCK", &sock);
    if !pid.is_empty() {
        cmd.env("SSH_AGENT_PID", &pid);
    }
    let _ = cmd.output(); // best-effort: -k prints `Agent pid … killed` on success

    let _ = std::fs::remove_file(&sock);
    crate::store::set_agent_record("", "");
    Ok(())
}

/// Add a key via `ssh-add <key>`. When the key needs a passphrase, either pass
/// one explicitly (it is fed through the askpass helper) or omit it and the UI
/// will be told `needs_passphrase=true` so it can prompt and retry.
#[tauri::command]
pub fn agent_add(
    state: State<'_, AgentState>,
    key: String,
    passphrase: Option<String>,
) -> Result<AddKeyResult, String> {
    if configured_socket().is_none() {
        return Err("no SSH agent running — start one in Tools → SSH key manager".into());
    }

    let mut cmd = std::process::Command::new("ssh-add");
    cmd.arg(&key);
    if let Some(s) = configured_socket() {
        cmd.env("SSH_AUTH_SOCK", &s);
    }

    let mut askpass_file: Option<PathBuf> = None;
    if let Some(pw) = passphrase.filter(|p| !p.is_empty()) {
        let (helper, file) = askpass_setup(&key, &pw);
        cmd.env("SSH_ASKPASS", &helper);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        // Any non-empty DISPLAY satisfies ssh's askpass engagement check.
        cmd.env("DISPLAY", ":0");
        cmd.env("NX_AGENT_ASKPASS_FILE", &file);
        askpass_file = Some(file);
    }

    let out = cmd.output().map_err(|e| format!("could not run ssh-add: {e}"))?;
    if let Some(f) = askpass_file {
        let _ = std::fs::remove_file(&f);
    }

    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let err2 = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        // Record the key path against its fingerprint so per-identity Remove
        // (`ssh-add -d <path>`) works — the agent itself only keeps fingerprints.
        if let Some(fp) = fingerprint_of(&key) {
            state.0.lock().unwrap().added.insert(fp, key.clone());
        }
        Ok(AddKeyResult {
            added: true,
            needs_passphrase: false,
            message: "Key added".into(),
        })
    } else if err.to_ascii_lowercase().contains("passphrase")
        || err2.to_ascii_lowercase().contains("passphrase") {
        Ok(AddKeyResult {
            added: false,
            needs_passphrase: true,
            message: format!("{err}{err2}").trim().to_string(),
        })
    } else {
        Err(format!("ssh-add failed: {err}{err2}"))
    }
}

/// Remove one identity (`ssh-add -d <path>`).
#[tauri::command]
pub fn agent_remove(state: State<'_, AgentState>, key: String) -> Result<(), String> {
    if configured_socket().is_none() {
        return Err("no SSH agent running".into());
    }
    let sock = configured_socket().unwrap_or_default();
    let (out, err, code) = run_ssh_add(vec!["-d".to_string(), key.clone()], sock);
    if code == 0 {
        // Forget the path (drop every entry pointing at it).
        let mut inner = state.0.lock().unwrap();
        let stale: Vec<String> = inner
            .added
            .iter()
            .filter(|(_, p)| **p == key)
            .map(|(fp, _)| fp.clone())
            .collect();
        for fp in stale {
            inner.added.remove(&fp);
        }
        Ok(())
    } else {
        Err(format!("ssh-add -d failed: {err}{out}"))
    }
}

/// Remove all identities (`ssh-add -D`).
#[tauri::command]
pub fn agent_remove_all() -> Result<(), String> {
    if configured_socket().is_none() {
        return Err("no SSH agent running".into());
    }
    let sock = configured_socket().unwrap_or_default();
    let (out, err, code) = run_ssh_add(vec!["-D".to_string()], sock);
    if code == 0 {
        Ok(())
    } else {
        Err(format!("ssh-add -D failed: {err}{out}"))
    }
}

/* ------------------------------ helpers ------------------------------ */

fn askpass_setup(id: &str, passphrase: &str) -> (PathBuf, PathBuf) {
    let dir = crate::store::config_dir();
    let helper = dir.join("agent-askpass.sh");
    if !helper.exists() {
        let _ = std::fs::write(&helper, "#!/bin/sh\ncat \"${NX_AGENT_ASKPASS_FILE}\"\n");
        let _ = std::fs::set_permissions(&helper, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    let file = dir.join(format!(".agent-askpass-{}", id.replace("/", "_").replace("\\", "_")));
    let _ = std::fs::write(&file, passphrase);
    let _ = std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    (helper, file)
}

/// Run `ssh-add` with the given args + socket, returning (stdout, stderr, exit code).
fn run_ssh_add(args: Vec<String>, sock: String) -> (String, String, i32) {
    let mut cmd = std::process::Command::new("ssh-add");
    for a in args {
        cmd.arg(&a);
    }
    cmd.env("SSH_AUTH_SOCK", &sock);
    match cmd.output() {
        Ok(out) => {
            let code = out.status.code().unwrap_or(-1);
            (
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
                code,
            )
        }
        Err(e) => ("".into(), format!("could not run ssh-add: {e}"), -1),
    }
}

/// Parse `ssh-add -l` output. Returns `(alive, keys)` — alive=false when the
/// agent could not be reached (stale socket, dead daemon, …).
pub fn parse_add_l(out: &str, err: &str, code: i32) -> (bool, Vec<AgentKey>) {
    let combined = format!("{out}\n{err}");
    if code != 0
        && (combined.contains("Could not open a connection")
            || combined.contains("Error connecting to agent")
            || combined.contains("No such file or directory")) {
        return (false, Vec::new());
    }
    let mut keys: Vec<AgentKey> = Vec::new();
    for line in out.split("\n") {
        let t = line.trim();
        if t.is_empty() || t.contains("no identities") || t.contains("Agent pid") {
            continue;
        }
        let tok: Vec<&str> = t.split_whitespace().collect();
        if tok.len() < 2 {
            continue;
        }
        let bits = tok[0].to_string();
        let fingerprint = tok[1].to_string();
        let mut key_type = String::with_capacity(0);
        let mut comment = String::with_capacity(0);
        if tok.len() >= 3 {
            let last = tok[tok.len() - 1];
            if last.starts_with('(') && last.ends_with(')') {
                key_type = last[1..last.len() - 1].to_string();
                comment = tok[2..tok.len() - 1].join(" ");
            } else {
                comment = tok[2..].join(" ");
            }
        }
        keys.push(AgentKey { bits, fingerprint, comment, key_type, source_path: "".into() });
    }
    (true, keys)
}

/// Compute the `SHA256:…` fingerprint of a local key file via `ssh-keygen -lf`
/// (same token layout as `ssh-add -l`, so `parse_add_l` doubles as parser).
fn fingerprint_of(path: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("ssh-keygen");
    cmd.arg("-lf").arg(path);
    match cmd.output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let (_, keys) = parse_add_l(&text, "", 0);
            if keys.is_empty() {
                None
            } else {
                Some(keys[0].fingerprint.clone())
            }
        }
        _ => None,
    }
}

/// Extract the numeric PID from ssh-agent's sh-style env output
/// (`SSH_AGENT_PID=12345; export SSH_AGENT_PID;`).
fn parse_agent_pid(out: &str) -> String {
    for line in out.split("\n") {
        let t = line.trim();
        if let Some(i) = t.find("SSH_AGENT_PID=") {
            let rest = &t[i + 14..];
            let j = rest.find(";").unwrap_or(rest.len());
            let pid = &rest[..j];
            if !pid.trim().is_empty() {
                return pid.trim().to_string();
            }
            return "".into();
        }
    }
    "".into()
}

/* --------------------------------- tests --------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fingerprint_comment_and_type() {
        let (alive, keys) = parse_add_l(
            "256 SHA256:abcdefghijklmnopqrstuvwxyz0123456789A sam@box (ED25519)\n",
            "",
            0,
        );
        assert!(alive);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].bits, "256");
        assert_eq!(keys[0].fingerprint, "SHA256:abcdefghijklmnopqrstuvwxyz0123456789A");
        assert_eq!(keys[0].comment, "sam@box");
        assert_eq!(keys[0].key_type, "ED25519");
    }

    #[test]
    fn parses_comment_with_spaces_and_no_type() {
        let (alive, keys) = parse_add_l("2048 SHA256:xx /home/u/.ssh/id_rsa\n", "", 0);
        assert!(alive);
        assert_eq!(keys[0].bits, "2048");
        assert_eq!(keys[0].comment, "/home/u/.ssh/id_rsa");
        assert_eq!(keys[0].key_type, "");
    }

    #[test]
    fn empty_agent_is_alive_without_keys() {
        let (alive, keys) = parse_add_l("The agent has no identities.\n", "", 1);
        assert!(alive);
        assert!(keys.is_empty());
    }

    #[test]
    fn dead_agent_is_not_alive() {
        let (alive, keys) = parse_add_l("", "Could not open a connection to your authentication agent.\n", 2);
        assert!(!alive);
        assert!(keys.is_empty());
        let (alive2, _) = parse_add_l("", "Error connecting to agent: No such file or directory\n", 2);
        assert!(!alive2);
    }

    #[test]
    fn extracts_agent_pid_from_sh_env_output() {
        let out = "SSH_AUTH_SOCK=/tmp/nx/agent.sock; export SSH_AUTH_SOCK;\nSSH_AGENT_PID=502591; export SSH_AGENT_PID;\necho Agent pid 502591;\n";
        assert_eq!(parse_agent_pid(out), "502591");
    }

    #[test]
    fn absent_environment_means_no_configured_socket_path() {
        // configured_socket must never panic when settings are empty.
        let _ = configured_socket();
    }

    /// End-to-end driver smoke test against a real `ssh-agent` spawned on a
    /// throwaway socket: start daemon, add an unencrypted throwaway key, list
    /// it, remove it, remove all, then `ssh-agent -k` and confirm the socket
    /// is gone. Run explicitly with:
    ///   cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
    #[test]
    #[ignore = "spawns a real ssh-agent + ssh-keygen on the local machine"]
    fn smoke_localhost_agent() {
        use std::time::Duration;
        let dir = std::env::temp_dir();
        let sock = dir.join("nx-agent-smoke.sock");
        let key = dir.join("nx-agent-smoke-key");
        let key_pub = dir.join("nx-agent-smoke-key.pub");
        let _ = std::fs::remove_file(&sock);
        let _ = std::fs::remove_file(&key);
        let _ = std::fs::remove_file(&key_pub);

        // Spawn the daemon the same way agent_start does.
        let mut cmd = std::process::Command::new("ssh-agent");
        cmd.arg("-a").arg(&sock.to_string_lossy().to_string());
        let out = cmd.output().expect("ssh-agent launch");
        assert!(out.status.success());
        let pid = parse_agent_pid(&String::from_utf8_lossy(&out.stdout).into_owned());
        assert!(!pid.is_empty(), "pid parse");
        assert!(sock.exists(), "socket created");
        let sock_str = sock.to_string_lossy().to_string();

        // Throwaway key (no passphrase).
        let mut kg = std::process::Command::new("ssh-keygen");
        kg.arg("-t").arg("ed25519").arg("-N").arg("");
        kg.arg("-f").arg(&key.to_string_lossy().to_string());
        let kgout = kg.output().expect("ssh-keygen");
        assert!(kgout.status.success(), "ssh-keygen: {}", String::from_utf8_lossy(&kgout.stderr).into_owned());

        // Add it with the socket env set.
        let mut add = std::process::Command::new("ssh-add");
        add.arg(&key.to_string_lossy().to_string());
        add.env("SSH_AUTH_SOCK", &sock_str);
        let aout = add.output().expect("ssh-add");
        assert!(aout.status.success(), "add: {}", String::from_utf8_lossy(&aout.stderr).into_owned());

        // List → 1 key.
        let (out2, err2, code2) = run_ssh_add(vec!["-l".to_string()], sock_str.clone());
        let (alive, keys) = parse_add_l(&out2, &err2, code2);
        assert!(alive);
        assert_eq!(keys.len(), 1, "one identity listed");
        assert_eq!(keys[0].key_type, "ED25519");

        // Remove one by path → empty.
        let (_, _, rc) = run_ssh_add(vec!["-d".to_string(), key.to_string_lossy().to_string()], sock_str.clone());
        assert_eq!(rc, 0);
        let (out3, err3, code3) = run_ssh_add(vec!["-l".to_string()], sock_str.clone());
        let (alive3, keys3) = parse_add_l(&out3, &err3, code3);
        assert!(alive3);
        assert!(keys3.is_empty());

        // Remove all → still empty and happy.
        let (_, _, rc2) = run_ssh_add(vec!["-D".to_string()], sock_str.clone());
        assert_eq!(rc2, 0);

        // Kill via `ssh-agent -k` with both env vars (as agent_stop does).
        let mut kill = std::process::Command::new("ssh-agent");
        kill.arg("-k");
        kill.env("SSH_AUTH_SOCK", &sock_str);
        kill.env("SSH_AGENT_PID", &pid);
        let kout = kill.output().expect("ssh-agent -k");
        assert!(kout.status.success(), "kill: {}", String::from_utf8_lossy(&kout.stderr).into_owned());
        std::thread::sleep(Duration::from_millis(300));
        assert!(!sock.exists(), "socket removed after -k");

        let _ = std::fs::remove_file(&key);
        let _ = std::fs::remove_file(&key_pub);
    }
}