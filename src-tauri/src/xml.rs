//! Reader/writer for the WinSSHTerm session-tree XML (`connections.xml` format).

use crate::model::{Node, KIND_CONTAINER};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use quick_xml::Reader;
use quick_xml::events::Event;

fn decode_container_name(raw: &str) -> String {
    // Containers store base64(UTF-8) names; fall back to the raw value if it isn't valid base64.
    match B64.decode(raw.as_bytes()) {
        Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|e| {
            String::from_utf8_lossy(e.as_bytes()).to_string()
        }),
        Err(_) => raw.to_string(),
    }
}

fn attr(map: &[(String, String)], key: &str) -> String {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// Parse a WinSSHTerm `connections.xml` document into a node tree.
pub fn parse_connections(xml: &str) -> Result<Vec<Node>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut roots: Vec<Node> = Vec::new();
    // Stack of (in-progress node, is_container)
    let mut stack: Vec<Node> = Vec::new();

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("XML parse error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                // Opening tag: parse attrs, then push onto the stack.
                if e.name().as_ref() != b"Node" {
                    continue;
                }
                let node = parse_node(&e);
                stack.push(node);
            }
            Ok(Event::Empty(e)) => {
                // Self-closing tag: parse attrs and attach immediately.
                if e.name().as_ref() != b"Node" {
                    continue;
                }
                let node = parse_node(&e);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    roots.push(node);
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() != b"Node" {
                    continue;
                }
                if let Some(done) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(done);
                    } else {
                        roots.push(done);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(roots)
}

fn parse_node(e: &quick_xml::events::BytesStart) -> Node {
    let mut raw: Vec<(String, String)> = Vec::new();
    for a in e.attributes().flatten() {
        let k = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let v = a
            .unescape_value()
            .map(|c| c.to_string())
            .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
        raw.push((k, v));
    }
    let node_type = attr(&raw, "Type");
    let mut node = Node {
        node_type: node_type.clone(),
        expanded: attr(&raw, "Expanded").eq_ignore_ascii_case("true"),
        ..Default::default()
    };
    if node_type.eq_ignore_ascii_case(KIND_CONTAINER) {
        node.name = decode_container_name(&attr(&raw, "Name"));
    } else {
        node.name = attr(&raw, "Name");
        node.descr = attr(&raw, "Descr");
        node.username = attr(&raw, "Username");
        node.password = attr(&raw, "Password");
        node.private_key = attr(&raw, "PrivateKey");
        node.hostname = attr(&raw, "Hostname");
        node.port = attr(&raw, "Port");
        node.certificate = attr(&raw, "Certificate");
        node.launch_tool = attr(&raw, "LaunchToolInt");
        node.x11 = attr(&raw, "sX11");
        node.cf_prot = attr(&raw, "cfProt");
        node.proxy_enabled = attr(&raw, "pSshProxy").eq_ignore_ascii_case("enabled");
        node.proxy_type = attr(&raw, "pType");
        node.proxy_host = attr(&raw, "pHost");
        node.proxy_port = attr(&raw, "pPort");
        node.proxy_user = attr(&raw, "pUser");
        node.proxy_telnet_cmd = attr(&raw, "pTelnetCmd");
    }
    node
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn write_node(out: &mut String, node: &Node, depth: usize) {
    let pad = "  ".repeat(depth);
    if node.is_container() {
        let b64 = B64.encode(node.name.as_bytes());
        out.push_str(&format!(
            "{pad}<Node Name='{b64}' Type='Container' Expanded='{}'>\n",
            if node.expanded { "True" } else { "False" }
        ));
        for c in &node.children {
            write_node(out, c, depth + 1);
        }
        out.push_str(&format!("{pad}</Node>\n"));
    } else {
        out.push_str(&format!(
            "{pad}<Node Name=\"{}\" Type=\"Connection\" Descr=\"{}\" Username=\"{}\" \
Password=\"{}\" PrivateKey=\"{}\" Hostname=\"{}\" Port=\"{}\" Certificate=\"{}\" \
LaunchToolInt=\"{}\" sX11=\"{}\" cfProt=\"{}\" pSshProxy=\"{}\" pType=\"{}\" pHost=\"{}\" \
pPort=\"{}\" pUser=\"{}\" pTelnetCmd=\"{}\" />\n",
            esc(&node.name),
            esc(&node.descr),
            esc(&node.username),
            esc(&node.password),
            esc(&node.private_key),
            esc(&node.hostname),
            esc(&node.port),
            esc(&node.certificate),
            esc(&node.launch_tool),
            esc(&node.x11),
            esc(&node.cf_prot),
            if node.proxy_enabled { "enabled" } else { "disabled" },
            esc(&node.proxy_type),
            esc(&node.proxy_host),
            esc(&node.proxy_port),
            esc(&node.proxy_user),
            esc(&node.proxy_telnet_cmd),
        ));
    }
}

/// Serialise a node tree into WinSSHTerm `connections.xml`.
pub fn write_connections(nodes: &[Node]) -> String {
    let mut out = String::from("<?xml version='1.0' encoding='utf-8'?>\n<WinSSHTerm Version='1'>\n");
    for n in nodes {
        write_node(&mut out, n, 1);
    }
    out.push_str("</WinSSHTerm>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    #[test]
    fn round_trip_preserves_tree() {
        let tree = vec![
            Node::container("Proxmox Hosts", true).with_children(vec![
                Node::connection("Elitedesk One", "192.168.100.201", "root", 22)
                    .with_key("/home/sam/.ssh/id_ed25519"),
                Node::connection("opnSense Local", "192.168.100.1", "admin", 443),
            ]),
            Node::connection("Debian WSL", "", "", 22),
        ];

        let xml = write_connections(&tree);
        let parsed = parse_connections(&xml).expect("parse should succeed");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Proxmox Hosts");
        assert_eq!(parsed[0].node_type, KIND_CONTAINER);
        assert!(parsed[0].expanded);
        assert_eq!(parsed[0].children.len(), 2);

        let first = &parsed[0].children[0];
        assert_eq!(first.name, "Elitedesk One");
        assert_eq!(first.hostname, "192.168.100.201");
        assert_eq!(first.username, "root");
        assert_eq!(first.port, "22");
        assert_eq!(first.private_key, "/home/sam/.ssh/id_ed25519");
        assert_eq!(first.cf_prot, "sftp");

        let second = &parsed[0].children[1];
        assert_eq!(second.name, "opnSense Local");
        assert_eq!(second.port, "443");

        assert_eq!(parsed[1].name, "Debian WSL");
        assert_eq!(parsed[1].hostname, "");
    }

    #[test]
    fn round_trip_is_stable() {
        // parse → write → parse must be idempotent.
        let tree = vec![Node::connection("Host", "10.0.0.1", "sam", 2222)];
        let once = write_connections(&tree);
        let parsed = parse_connections(&once).unwrap();
        let twice = write_connections(&parsed);
        assert_eq!(once, twice);
    }

    #[test]
    fn escapes_special_chars() {
        let tree = vec![Node::connection("A&B <C> \"D\"", "h", "u", 22)];
        let xml = write_connections(&tree);
        let parsed = parse_connections(&xml).unwrap();
        assert_eq!(parsed[0].name, "A&B <C> \"D\"");
    }
}