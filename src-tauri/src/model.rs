//! Session tree data model — mirrors the WinSSHTerm session XML exactly.
//!
//! Schema (verified against Migrate2WinSSHTerm v0.23):
//!   <Node Name='<base64 path seg>' Type='Container' Expanded='True'>
//!     <Node Name="label" Type="Connection" Descr="" Username="" Password=""
//!           PrivateKey="" Hostname="" Port="" Certificate="" LaunchToolInt=""
//!           sX11="" cfProt="" pSshProxy="enabled|disabled" pType="" pHost=""
//!           pPort="" pUser="" pTelnetCmd="" />
//!   </Node>

use serde::{Deserialize, Serialize};

pub const KIND_CONTAINER: &str = "Container";
pub const KIND_CONNECTION: &str = "Connection";

/// A node in the session tree: either a folder (Container) or a host (Connection).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Node {
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub children: Vec<Node>,

    // ---- Connection attributes (WinSSHTerm names in comments) ----
    #[serde(default)]
    pub descr: String, // Descr
    #[serde(default)]
    pub username: String, // Username
    #[serde(default)]
    pub password: String, // Password (vault-backed once unlocked)
    #[serde(default)]
    pub private_key: String, // PrivateKey
    #[serde(default)]
    pub hostname: String, // Hostname
    #[serde(default)]
    pub port: String, // Port
    #[serde(default)]
    pub certificate: String, // Certificate
    #[serde(default)]
    pub launch_tool: String, // LaunchToolInt
    #[serde(default)]
    pub x11: String, // sX11
    #[serde(default)]
    pub cf_prot: String, // cfProt
    #[serde(default)]
    pub proxy_enabled: bool, // pSshProxy == "enabled"
    #[serde(default)]
    pub proxy_type: String, // pType
    #[serde(default)]
    pub proxy_host: String, // pHost
    #[serde(default)]
    pub proxy_port: String, // pPort
    #[serde(default)]
    pub proxy_user: String, // pUser
    #[serde(default)]
    pub proxy_telnet_cmd: String, // pTelnetCmd

    // ---- UI-local (not persisted to WinSSHTerm XML) ----
    #[serde(default)]
    pub login_dir: String,       // Login Dir
    #[serde(default)]
    pub login_cmds: String,      // Login Cmds
    #[serde(default)]
    pub cmdline_args: String,    // Cmd-line Args
    #[serde(default)]
    pub env_color: String,       // Env Color
    #[serde(default)]
    pub custom_id: String,       // Custom Id
    #[serde(default)]
    pub custom_type: String,     // Custom Type
}

impl Node {
    pub fn container(name: &str, expanded: bool) -> Self {
        Self {
            name: name.to_string(),
            node_type: KIND_CONTAINER.into(),
            expanded,
            ..Default::default()
        }
    }

    pub fn connection(name: &str, hostname: &str, username: &str, port: u16) -> Self {
        Self {
            name: name.to_string(),
            node_type: KIND_CONNECTION.into(),
            hostname: hostname.to_string(),
            username: username.to_string(),
            port: port.to_string(),
            cf_prot: "sftp".into(),
            x11: "don't forward".into(),
            ..Default::default()
        }
    }

    pub fn is_container(&self) -> bool {
        self.node_type == KIND_CONTAINER
    }

    #[allow(dead_code)] // used by the importer preview (Phase 2)
    pub fn port_or(&self, default: u16) -> u16 {
        self.port.parse::<u16>().unwrap_or(default)
    }

    pub fn with_children(mut self, kids: Vec<Node>) -> Self {
        self.children = kids;
        self
    }

    #[allow(dead_code)] // used by tests and the importer (Phase 2)
    pub fn with_key(mut self, key: &str) -> Self {
        self.private_key = key.to_string();
        self
    }
}

/// Flatten the tree into `(path, node)` pairs — handy for search and for the importer preview.
pub fn flatten(nodes: &[Node], prefix: &[String], out: &mut Vec<(Vec<String>, Node)>) {
    for n in nodes {
        let mut path = prefix.to_vec();
        path.push(n.name.clone());
        out.push((path.clone(), n.clone()));
        if !n.children.is_empty() {
            flatten(&n.children, &path, out);
        }
    }
}

/// Recursively clear every node's password. Used before writing to disk so
/// plaintext passwords are never persisted (vault lands in v0.2).
pub fn strip_passwords(nodes: &mut [Node]) {
    for n in nodes {
        n.password.clear();
        strip_passwords(&mut n.children);
    }
}