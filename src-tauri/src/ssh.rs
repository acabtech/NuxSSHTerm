//! SSH launch specification and command-line construction.
//!
//! v0.1 uses the system OpenSSH client over a PTY. This means we inherit everything
//! the user already has: ~/.ssh/config, ssh-agent, known_hosts, jump hosts, X11.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaunchSpec {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub private_key: String,
    #[serde(default)]
    pub x11: bool,
    #[serde(default)]
    pub forward_agent: bool,
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Raw WinSSHTerm proxy settings, mapped onto ssh -o ProxyCommand where possible.
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default)]
    pub proxy_type: String,
    #[serde(default)]
    pub proxy_host: String,
    #[serde(default)]
    pub proxy_port: String,
    #[serde(default)]
    pub proxy_telnet_cmd: String,
}

fn default_port() -> u16 {
    22
}

/// Bare OpenSSH flags imported sessions may pass through. Anything outside this
/// list is dropped: `extra_args` comes from imported XML/JSON, so it is untrusted
/// argv (issue #44).
const SAFE_BARE_FLAGS: &[&str] = &[
    "-4", "-6", "-C", "-N", "-T", "-Y", "-q", "-v", "-vv", "-vvv", "-vvvv",
];

const NUMERIC_OPTIONS: &[&str] = &[
    "connectionattempts",
    "connecttimeout",
    "numberofpasswordprompts",
    "serveraliveinterval",
    "serveralivecountmax",
];

const YES_NO_OPTIONS: &[&str] = &[
    "batchmode",
    "compression",
    "exitonforwardfailure",
    "tcpkeepalive",
];

const LOG_LEVELS: &[&str] = &[
    "quiet", "fatal", "error", "info", "verbose", "debug", "debug1", "debug2", "debug3",
];

fn is_safe_option(option: &str) -> bool {
    let Some((key, value)) = option.split_once('=') else {
        return false;
    };
    if key.is_empty()
        || key
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() || c.is_control())
        || value.is_empty()
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let key = key.to_ascii_lowercase();
    if NUMERIC_OPTIONS.contains(&key.as_str()) {
        return value.len() <= 10 && value.bytes().all(|b| b.is_ascii_digit());
    }
    if YES_NO_OPTIONS.contains(&key.as_str()) {
        return value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("no");
    }
    if key == "loglevel" {
        return LOG_LEVELS.contains(&value.to_ascii_lowercase().as_str());
    }
    false
}

/// Filter imported `extra_args` down to forms that cannot reconfigure OpenSSH
/// into running local commands (`ProxyCommand`, `LocalCommand`, custom
/// `ControlPath`, host-key disabling, batch files, etc.).
fn safe_extra_args(args: &[String]) -> Vec<String> {
    let mut safe = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw.chars().any(char::is_control) {
            i += 1;
            continue;
        }
        let arg = raw.trim();
        if arg.is_empty() {
            i += 1;
            continue;
        }
        if arg == "-o" {
            // `-o Key=Value` and `-oKey=Value` are both accepted by OpenSSH;
            // consume the following token either way so an unsafe value can
            // never be reinterpreted as a bare flag or destination.
            if let Some(next) = args.get(i + 1) {
                if !next.chars().any(char::is_control) {
                    let next = next.trim();
                    if is_safe_option(next) {
                        safe.push("-o".into());
                        safe.push(next.to_string());
                    }
                }
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(option) = arg.strip_prefix("-o") {
            if !option.is_empty() && is_safe_option(option) {
                safe.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        if SAFE_BARE_FLAGS.contains(&arg) {
            safe.push(arg.to_string());
        }
        i += 1;
    }
    safe
}

impl LaunchSpec {
    /// Build the `ssh` argv for this spec (program name excluded).
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        if self.port != 0 && self.port != 22 {
            args.push("-p".into());
            args.push(self.port.to_string());
        }
        if !self.username.is_empty() {
            args.push("-l".into());
            args.push(self.username.clone());
        }
        if !self.private_key.is_empty() {
            args.push("-i".into());
            args.push(self.private_key.clone());
        }
        if self.x11 {
            // -Y is the pragmatic choice for a desktop client; PuTTY's equivalent
            // ("enable X11 forwarding") behaves like -X but most users expect -Y.
            args.push("-Y".into());
        }
        if self.forward_agent {
            args.push("-o".into());
            args.push("ForwardAgent=yes".into());
        }
        if self.proxy_enabled
            && let Some(pc) = self.proxy_command()
        {
            args.push("-o".into());
            args.push(format!("ProxyCommand={pc}"));
        }
        args.extend(safe_extra_args(&self.extra_args));
        args.push(self.host.clone());
        args
    }

    /// Map WinSSHTerm proxy types onto an OpenSSH ProxyCommand.
    /// Requires `nc` (netcat) for SOCKS/HTTP — flagged in the UI when missing.
    fn proxy_command(&self) -> Option<String> {
        let hostport = format!("{}:{}", self.proxy_host, self.proxy_port);
        match self.proxy_type.to_ascii_uppercase().as_str() {
            "SOCKS4" => Some(format!("nc -X 4 -x {hostport} %h %p")),
            "SOCKS5" => Some(format!("nc -X 5 -x {hostport} %h %p")),
            "HTTP" => Some(format!("nc -X connect -x {hostport} %h %p")),
            // "Local" = a custom telnet/command proxy; WinSSHTerm stores the raw command.
            "LOCAL" => {
                let cmd = self.proxy_telnet_cmd.trim();
                if cmd.is_empty() {
                    None
                } else {
                    Some(cmd.replace("%host", "%h").replace("%port", "%p"))
                }
            }
            _ => None,
        }
    }

    /// Build the `sftp` argv for this spec (program name excluded).
    ///
    /// Purposely NOT `ssh_args()`: sftp's flags differ (port is `-P`, not `-p`;
    /// `-l` means bandwidth-limit, so the user goes through `-o User=`), X11
    /// forwarding is meaningless, and the `-p`/`-l` forms would misfire.
    pub fn sftp_args(&self) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        if self.port != 0 && self.port != 22 {
            args.push("-P".into());
            args.push(self.port.to_string());
        }
        if !self.username.is_empty() {
            args.push("-o".into());
            args.push(format!("User={}", self.username));
        }
        if !self.private_key.is_empty() {
            args.push("-i".into());
            args.push(self.private_key.clone());
        }
        if self.forward_agent {
            args.push("-o".into());
            args.push("ForwardAgent=yes".into());
        }
        if self.proxy_enabled
            && let Some(pc) = self.proxy_command()
        {
            args.push("-o".into());
            args.push(format!("ProxyCommand={pc}"));
        }
        args.extend(safe_extra_args(&self.extra_args));
        args.push(self.host.clone());
        args
    }

    /// Human-readable one-liner, shown in the tab tooltip / status bar.
    #[allow(dead_code)] // surfaced via the frontend's `targetOf`; kept for parity/tests
    pub fn display_target(&self) -> String {
        let user = if self.username.is_empty() {
            "?".to_string()
        } else {
            self.username.clone()
        };
        format!("{user}@{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_spec_is_just_host() {
        let s = LaunchSpec {
            host: "example.com".into(),
            ..Default::default()
        };
        assert_eq!(s.ssh_args(), vec!["example.com"]);
    }

    #[test]
    fn port_username_and_key() {
        let s = LaunchSpec {
            host: "h".into(),
            port: 2222,
            username: "sam".into(),
            private_key: "/home/user/.ssh/id_ed25519".into(),
            ..Default::default()
        };
        assert_eq!(
            s.ssh_args(),
            vec![
                "-p",
                "2222",
                "-l",
                "sam",
                "-i",
                "/home/user/.ssh/id_ed25519",
                "h"
            ]
        );
    }

    #[test]
    fn default_port_22_is_omitted() {
        let s = LaunchSpec {
            host: "h".into(),
            port: 22,
            ..Default::default()
        };
        assert_eq!(s.ssh_args(), vec!["h"]);
    }

    #[test]
    fn x11_and_agent_forwarding() {
        let s = LaunchSpec {
            host: "h".into(),
            x11: true,
            forward_agent: true,
            ..Default::default()
        };
        assert_eq!(s.ssh_args(), vec!["-Y", "-o", "ForwardAgent=yes", "h"]);
    }

    #[test]
    fn socks5_proxy_command() {
        let s = LaunchSpec {
            host: "h".into(),
            proxy_enabled: true,
            proxy_type: "SOCKS5".into(),
            proxy_host: "proxy.local".into(),
            proxy_port: "1080".into(),
            ..Default::default()
        };
        assert_eq!(
            s.ssh_args(),
            vec!["-o", "ProxyCommand=nc -X 5 -x proxy.local:1080 %h %p", "h"]
        );
    }

    #[test]
    fn local_proxy_uses_telnet_command() {
        let s = LaunchSpec {
            host: "h".into(),
            proxy_enabled: true,
            proxy_type: "Local".into(),
            proxy_telnet_cmd: "nc %host %port".into(),
            ..Default::default()
        };
        assert_eq!(s.ssh_args(), vec!["-o", "ProxyCommand=nc %h %p", "h"]);
    }

    #[test]
    fn sftp_args_use_sftp_flag_conventions() {
        let s = LaunchSpec {
            host: "h".into(),
            port: 2222,
            username: "sam".into(),
            private_key: "/home/user/.ssh/id_ed25519".into(),
            ..Default::default()
        };
        assert_eq!(
            s.sftp_args(),
            vec![
                "-P",
                "2222",
                "-o",
                "User=sam",
                "-i",
                "/home/user/.ssh/id_ed25519",
                "h"
            ]
        );
    }

    #[test]
    fn sftp_args_omit_default_port_and_proxy() {
        let s = LaunchSpec {
            host: "h".into(),
            proxy_enabled: true,
            proxy_type: "SOCKS5".into(),
            proxy_host: "proxy.local".into(),
            proxy_port: "1080".into(),
            ..Default::default()
        };
        assert_eq!(
            s.sftp_args(),
            vec!["-o", "ProxyCommand=nc -X 5 -x proxy.local:1080 %h %p", "h"]
        );
    }

    #[test]
    fn sftp_args_do_not_emit_x11_flag() {
        let s = LaunchSpec {
            host: "h".into(),
            x11: true,
            ..Default::default()
        };
        assert_eq!(s.sftp_args(), vec!["h"]);
    }

    #[test]
    fn extra_args_are_appended_before_host() {
        let s = LaunchSpec {
            host: "h".into(),
            extra_args: vec!["-o".into(), "ServerAliveInterval=30".into()],
            ..Default::default()
        };
        assert_eq!(s.ssh_args(), vec!["-o", "ServerAliveInterval=30", "h"]);
    }

    #[test]
    fn dangerous_extra_args_are_dropped() {
        let s = LaunchSpec {
            host: "h".into(),
            extra_args: vec![
                "-o".into(),
                "ProxyCommand=/bin/sh -c 'touch /tmp/pwned'".into(),
                "-oProxyCommand=/bin/sh -c 'touch /tmp/pwned'".into(),
                "-o".into(),
                "LocalCommand=id".into(),
                "-oPermitLocalCommand=yes".into(),
                "-o".into(),
                "StrictHostKeyChecking=no".into(),
                "-b".into(),
                "junk".into(),
                "-v".into(),
            ],
            ..Default::default()
        };
        let expected = vec!["-v", "h"];
        assert_eq!(s.ssh_args(), expected);
        assert_eq!(s.sftp_args(), expected);
    }

    #[test]
    fn safe_compact_options_and_flags_survive_filtering() {
        let s = LaunchSpec {
            host: "h".into(),
            extra_args: vec![
                "-oServerAliveCountMax=3".into(),
                "-o".into(),
                "LogLevel=DEBUG2".into(),
                "-oConnectTimeout=not-a-number".into(),
                "-o".into(),
                "Compression=maybe".into(),
                "-C".into(),
                "-vv".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            s.ssh_args(),
            vec![
                "-oServerAliveCountMax=3",
                "-o",
                "LogLevel=DEBUG2",
                "-C",
                "-vv",
                "h"
            ]
        );
    }

    #[test]
    fn malformed_and_control_character_extra_args_are_dropped() {
        let s = LaunchSpec {
            host: "h".into(),
            extra_args: vec![
                "-v\n-oProxyCommand=id".into(),
                "-o".into(),
                "ServerAliveInterval=30\nLocalCommand=id".into(),
                "-o".into(),
                "-v".into(),
                "-o".into(),
                "ServerAliveInterval=30".into(),
                "-vvv".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            s.ssh_args(),
            vec!["-o", "ServerAliveInterval=30", "-vvv", "h"]
        );
    }
}
