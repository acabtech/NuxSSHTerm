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
            && let Some(pc) = self.proxy_command() {
                args.push("-o".into());
                args.push(format!("ProxyCommand={pc}"));
            }
        for extra in &self.extra_args {
            if !extra.trim().is_empty() {
                args.push(extra.clone());
            }
        }
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
            private_key: "/home/sam/.ssh/id_ed25519".into(),
            ..Default::default()
        };
        assert_eq!(
            s.ssh_args(),
            vec!["-p", "2222", "-l", "sam", "-i", "/home/sam/.ssh/id_ed25519", "h"]
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
    fn extra_args_are_appended_before_host() {
        let s = LaunchSpec {
            host: "h".into(),
            extra_args: vec!["-o".into(), "ServerAliveInterval=30".into()],
            ..Default::default()
        };
        assert_eq!(s.ssh_args(), vec!["-o", "ServerAliveInterval=30", "h"]);
    }
}