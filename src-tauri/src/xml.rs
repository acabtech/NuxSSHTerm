//! Reader/writer for the WinSSHTerm session-tree XML (`connections.xml` format),
//! plus the compressed `<WinSSHTerm_Backup>` format (base64(gzip(xml)) chunks).

use crate::model::{Node, KIND_CONTAINER};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use flate2::bufread::GzDecoder;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::prelude::*;

fn decode_container_name(raw: &str) -> String {
    // Some WinSSHTerm versions store container names as base64(UTF-8); real
    // exports usually store the name verbatim. Only accept a base64 decode when
    // it yields non-empty, printable UTF-8 — otherwise keep the raw value.
    if let Ok(bytes) = B64.decode(raw.as_bytes()) {
        if let Ok(s) = String::from_utf8(bytes) {
            if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                return s;
            }
        }
    }
    raw.to_string()
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

/// Parse a WinSSHTerm compressed backup (`<WinSSHTerm_Backup>` with `<Data><c n="..">` chunks).
///
/// WinSSHTerm's backup stores the session tree as `base64(gzip(connections.xml))` split into
/// numbered `<c>` elements. Reassemble the chunks in document order, base64-decode, gunzip,
/// then parse the inner XML with [`parse_connections`].
pub fn parse_backup(xml: &str) -> Result<Vec<Node>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    // base64 text of each <c> chunk, in document order.
    let mut chunks: Vec<String> = Vec::new();
    let mut in_c = false;
    let mut cur_text = String::new();

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("XML parse error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == b"c" {
                    cur_text.clear();
                    in_c = true;
                }
            }
            Ok(Event::Text(t)) => {
                if in_c {
                    let s = t.unescape().map(|c| c.to_string()).unwrap_or_default();
                    cur_text.push_str(&s);
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"c" && in_c {
                    chunks.push(cur_text.clone());
                    in_c = false;
                }
            }
            _ => {}
        }
    }

    if chunks.is_empty() {
        return Err("no <c> data chunks found in the WinSSHTerm backup".into());
    }

    // Reassemble the base64 payload.
    let mut b64 = String::with_capacity(0);
    for part in chunks {
        b64.push_str(&part);
    }

    // base64 → gzip bytes.
    let compressed = B64.decode(b64.as_bytes())
        .map_err(|e| format!("backup payload is not valid base64: {e}"))?;

    // gzip → the backup payload.
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut plain: Vec<u8> = Vec::new();
    let _ = decoder
        .read_to_end(&mut plain)
        .map_err(|e| format!("backup payload is not valid gzip data: {e}"))?;

    // The payload is normally a protobuf envelope (see [`protobuf_field1`]);
    // some older/simple payloads are the connections.xml document directly.
    let inner = if looks_like_xml(&plain) {
        strip_bom(&String::from_utf8_lossy(&plain)).to_string()
    } else {
        let field1 = protobuf_field1(&plain).ok_or_else(|| {
            "backup payload is not a recognised WinSSHTerm envelope".to_string()
        })?;
        let xml_bytes = B64
            .decode(field1)
            .map_err(|e| format!("backup session data is not valid base64: {e}"))?;
        strip_bom(&String::from_utf8_lossy(&xml_bytes)).to_string()
    };

    let nodes = parse_connections(&inner)?;
    if nodes.is_empty() {
        return Err("no sessions found in the decompressed WinSSHTerm backup".into());
    }
    Ok(nodes)
}

/// True when `bytes` looks like an XML document (optionally with a UTF-8 BOM).
fn looks_like_xml(bytes: &[u8]) -> bool {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let start = b
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(b.len());
    let b = &b[start..];
    b.starts_with(b"<?xml") || b.starts_with(b"<WinSSHTerm") || b.starts_with(b"<Node")
}

/// Strip a leading UTF-8 BOM (quick-xml does not treat it as whitespace).
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Read a protobuf base-128 varint at `i`, returning `(value, next_index)`.
fn read_varint(b: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut shift = 0u32;
    let mut val = 0u64;
    loop {
        if i >= b.len() || shift >= 64 {
            return None;
        }
        let x = b[i];
        i += 1;
        val |= ((x & 0x7f) as u64) << shift;
        if x & 0x80 == 0 {
            return Some((val, i));
        }
        shift += 7;
    }
}

/// Extract the value of length-delimited field number 1 from a protobuf message.
///
/// WinSSHTerm's compressed backup wraps the session tree in a protobuf envelope:
///   field 1 = base64(UTF-8 `connections.xml`)   ← the session tree
///   field 2 = base64(UTF-8 settings XML)
///   field 3 = signature bytes
///   field 4 = base64(UTF-16LE dock-panel layout)
fn protobuf_field1(msg: &[u8]) -> Option<&[u8]> {
    let mut i = 0usize;
    while i < msg.len() {
        let (tag, ni) = read_varint(msg, i)?;
        i = ni;
        let field = tag >> 3;
        let wire = tag & 7;
        match wire {
            2 => {
                let (len, ni) = read_varint(msg, i)?;
                i = ni;
                let end = i.checked_add(len as usize)?;
                if end > msg.len() {
                    return None;
                }
                if field == 1 {
                    return Some(&msg[i..end]);
                }
                i = end;
            }
            0 => {
                let (_v, ni) = read_varint(msg, i)?;
                i = ni;
            }
            1 => i = i.checked_add(8)?,
            5 => i = i.checked_add(4)?,
            _ => return None,
        }
    }
    None
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
    use flate2::write::GzEncoder;
    use flate2::Compression;

    #[test]
    fn round_trip_preserves_tree() {
        let tree = vec![
            Node::container("Proxmox Hosts", true).with_children(vec![
                Node::connection("Elitedesk One", "192.168.100.201", "root", 22)
                    .with_key("/home/user/.ssh/id_ed25519"),
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
        assert_eq!(first.private_key, "/home/user/.ssh/id_ed25519");
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

    /// Wrap an arbitrary payload as a WinSSHTerm compressed backup: gzip + base64,
    /// split into small `<c>` chunks exactly like the real backup format.
    fn wrap_backup(payload: &[u8]) -> String {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(payload).unwrap();
        let compressed = enc.finish().unwrap();

        let b64 = B64.encode(&compressed);

        let mut data = String::new();
        let mut i = 0;
        let step = 100;
        let mut n = 0;
        while i < b64.len() {
            let end = (i + step).min(b64.len());
            data.push_str(&format!("        <c n=\"{n}\">{}</c>\n", &b64[i..end]));
            i = end;
            n += 1;
        }

        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<WinSSHTerm_Backup Version=\"1\">\n    <CreationDate>1/1/2026 12:00:00 AM</CreationDate>\n    <Data>\n{data}    </Data>\n</WinSSHTerm_Backup>"
        )
    }

    /// Build a WinSSHTerm compressed backup from a node tree (payload = raw XML).
    fn make_backup(tree: Vec<Node>) -> String {
        wrap_backup(write_connections(&tree).as_bytes())
    }

    /// Append a protobuf base-128 varint to `out`.
    fn write_varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let mut byte = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if v == 0 {
                break;
            }
        }
    }

    /// Build a backup using the *real* WinSSHTerm envelope: the gzip payload is a
    /// protobuf message whose field 1 is base64(connections.xml).
    fn make_envelope_backup(tree: Vec<Node>) -> String {
        let inner = write_connections(&tree);
        let field1 = B64.encode(inner.as_bytes());

        let mut msg: Vec<u8> = Vec::new();
        msg.push(0x0a); // field 1, wire type 2 (length-delimited)
        write_varint(&mut msg, field1.len() as u64);
        msg.extend_from_slice(field1.as_bytes());
        // A trailing field 2 to prove the parser skips unknown fields.
        msg.push(0x12); // field 2, wire type 2
        write_varint(&mut msg, 3);
        msg.extend_from_slice(b"abc");

        wrap_backup(&msg)
    }

    #[test]
    fn parses_compressed_backup() {
        let tree = vec![
            Node::container("Proxmox Hosts", true).with_children(vec![
                Node::connection("Elitedesk One", "192.168.100.201", "root", 22)
                    .with_key("/home/user/.ssh/id_ed25519"),
            ]),
            Node::connection("Debian WSL", "", "", 22),
        ];

        let backup = make_backup(tree);
        let parsed = parse_backup(&backup).expect("compressed backup should parse");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Proxmox Hosts");
        assert_eq!(parsed[0].node_type, KIND_CONTAINER);
        assert_eq!(parsed[0].children.len(), 1);
        let first = &parsed[0].children[0];
        assert_eq!(first.name, "Elitedesk One");
        assert_eq!(first.hostname, "192.168.100.201");
        assert_eq!(first.private_key, "/home/user/.ssh/id_ed25519");
        assert_eq!(parsed[1].name, "Debian WSL");
    }

    #[test]
    fn parses_protobuf_envelope_backup() {
        // The real WinSSHTerm backup wraps the XML in a protobuf envelope; the
        // old code fed the raw protobuf bytes to the XML parser and found nothing.
        let tree = vec![
            Node::container("Proxmox Hosts", true).with_children(vec![
                Node::connection("Elitedesk One", "192.168.100.201", "root", 22)
                    .with_key("/home/user/.ssh/id_ed25519"),
            ]),
            Node::connection("Debian WSL", "", "", 22),
        ];

        let backup = make_envelope_backup(tree);
        let parsed = parse_backup(&backup).expect("protobuf envelope backup should parse");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Proxmox Hosts");
        assert_eq!(parsed[0].node_type, KIND_CONTAINER);
        assert_eq!(parsed[0].children.len(), 1);
        let first = &parsed[0].children[0];
        assert_eq!(first.name, "Elitedesk One");
        assert_eq!(first.hostname, "192.168.100.201");
        assert_eq!(first.private_key, "/home/user/.ssh/id_ed25519");
        assert_eq!(parsed[1].name, "Debian WSL");
    }

    #[test]
    fn container_names_are_plain_or_base64() {
        // Real exports store plain names; the writer stores base64. Both must
        // round-trip, and a plain name that happens to be valid base64 ("ACAB")
        // must not be corrupted.
        assert_eq!(decode_container_name("Dev Hosts"), "Dev Hosts");
        assert_eq!(decode_container_name("ACAB"), "ACAB");
        assert_eq!(decode_container_name("CDA"), "CDA");
        assert_eq!(decode_container_name(&B64.encode("Proxmox Hosts")), "Proxmox Hosts");
    }

    /// Optional end-to-end check against a real WinSSHTerm export. Set
    /// `WINSSHTERM_BACKUP=/path/to/WinSSHTerm.xml` to run it; otherwise it is a
    /// no-op so the suite stays portable.
    #[test]
    fn parses_real_backup_from_env() {
        let Ok(path) = std::env::var("WINSSHTERM_BACKUP") else {
            return;
        };
        let bytes = std::fs::read(&path).expect("read WINSSHTERM_BACKUP");
        let text = String::from_utf8_lossy(&bytes);
        let nodes = parse_backup(&text).expect("real backup should parse");
        assert!(!nodes.is_empty(), "real backup should contain sessions");
    }

    #[test]
    fn rejects_corrupt_backup_payload() {
        // Valid base64 that is not gzip → a clear error, not a panic.
        let backup = "<?xml version=\"1.0\"?><WinSSHTerm_Backup Version=\"1\"><Data><c n=\"0\">aGVsb2N0</c></Data></WinSSHTerm_Backup>";
        let res = parse_backup(&backup);
        assert!(res.is_err());
    }
}