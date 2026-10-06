//! Best-effort KeePass `.kdbx` importer (Phase 2, optional).
//!
//! Shells out to `keepassxc-cli export -f xml <db>` and maps group titles +
//! entry `Title/UserName/Password/URL` fields onto the session tree. Entries
//! without a URL are skipped (they are typically notes, not hosts), and their
//! passwords are never imported. Passwords of imported entries are returned
//! separately so the caller can place them in the encrypted vault.
//!
//! Password-protected databases that cannot be opened non-interactively (the
//! export reads stdin, which is closed here) produce a guidance warning.

use crate::model::Node;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;

/// Intermediate in-memory group while walking the export XML.
struct Group {
    name: String,
    entries: Vec<Entry>,
    groups: Vec<Group>,
}

struct Entry {
    title: String,
    host: String,
    user: String,
    pass: String,
}

/// True when `keepassxc-cli` is available on `PATH` (needed for `.kdbx` import).
#[tauri::command]
pub fn keepassxc_available() -> bool {
    crate::importcmd::program_on_path("keepassxc-cli")
}

/// Run `keepassxc-cli export -f xml` against a database path.
pub fn export_xml(db_path: &str) -> Result<String, String> {
    let mut cmd = std::process::Command::new("keepassxc-cli");
    cmd.arg("export").arg("-f").arg("xml").arg(db_path);
    let out = cmd
        .output()
        .map_err(|e| format!("keepassxc-cli could not be started: {e}"))?;
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
        let err: String = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(format!(
            "keepassxc-cli export failed (exit {code}): {err}. Open the database in KeePassXC \
or export it as XML manually (Menu → Database → Export → XML), then import that file."
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn field(map: &HashMap<String, String>, key: &str) -> String {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn build(
    nodes: &mut Vec<Node>,
    group: &Group,
    key_prefix: &str,
    warnings: &mut Vec<String>,
    passwords: &mut HashMap<String, String>,
) {
    let group_key = if key_prefix.is_empty() {
        group.name.clone()
    } else {
        let group_name = group.name.clone();
        format!("{key_prefix}/{group_name}")
    };
    let mut node = Node::container(&group.name, true);

    for e in &group.entries {
        let title = e.title.clone();
        if e.host.is_empty() {
            let gname = group.name.clone();
            warnings.push(format!(
                "Skipped '{title}' (no URL/host in {gname}) — entries without a host are not imported."
            ));
            continue;
        }
        if !e.pass.is_empty() {
            passwords.insert(format!("{group_key}/{title}"), e.pass.clone());
        }
        node.children.push(Node::connection(&title, &e.host, &e.user, 22));
    }
    for sub in &group.groups {
        build(&mut node.children, sub, &group_key, warnings, passwords);
    }
    nodes.push(node);
}

/// (session tree, vault passwords, warnings)
type ParsedExport = (Vec<Node>, HashMap<String, String>, Vec<String>);

/// Parse a KeePass XML export. Returns (session tree, vault passwords, warnings).
pub fn parse_xml(text: &str) -> Result<ParsedExport, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);

    let mut tags: Vec<String> = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut roots: Vec<Group> = Vec::new();

    let mut entry_fields: Option<HashMap<String, String>> = None;
    let mut pending_key: Option<String> = None;
    let mut text_buf = String::with_capacity(0);
    let mut ignore_depth = 0;

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("KeePass XML parse error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                text_buf = String::with_capacity(0);
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "History" || tag == "DeletedObjects" {
                    ignore_depth += 1;
                }
                tags.push(tag.clone());
                if ignore_depth > 0 {
                    continue;
                }
                if tag == "Group" {
                    groups.push(Group { name: "".into(), entries: Vec::new(), groups: Vec::new() });
                } else if tag == "Entry" {
                    entry_fields = Some(HashMap::new());
                }
            }
            Ok(Event::Text(e)) => {
                if let Ok(t) = e.unescape() {
                    text_buf.push_str(t.as_ref());
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "History" || tag == "DeletedObjects" {
                    ignore_depth -= 1;
                }
                tags.pop();
                if ignore_depth > 0 {
                    text_buf = String::with_capacity(0);
                    continue;
                }
                match tag.as_str() {
                    "Key" => {
                        pending_key = Some(text_buf.clone());
                    }
                    "Value" => {
                        if let Some(k) = pending_key.take()
                            && let Some(ref mut fields) = entry_fields {
                                fields.insert(k, text_buf.clone());
                            }
                    }
                    "Name" => {
                        // KeePass group titles; entry titles come via String blocks.
                        if let Some(g) = groups.last_mut() {
                            g.name = text_buf.clone();
                        }
                    }
                    "Entry" => {
                        if let Some(fields) = entry_fields.take()
                            && let Some(g) = groups.last_mut() {
                                let mut title = field(&fields, "Title");
                                if title.is_empty() {
                                    title = "Unnamed".into();
                                }
                                let e = Entry {
                                    title,
                                    host: field(&fields, "URL"),
                                    user: field(&fields, "UserName"),
                                    pass: field(&fields, "Password"),
                                };
                                g.entries.push(e);
                            }
                    }
                    "Group" => {
                        if let Some(g) = groups.pop() {
                            if let Some(parent) = groups.last_mut() {
                                parent.groups.push(g);
                            } else {
                                roots.push(g);
                            }
                        }
                    }
                    _ => {}
                }
                text_buf = String::with_capacity(0);
            }
            _ => {}
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut passwords: HashMap<String, String> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();
    for g in roots {
        build(&mut nodes, &g, "", &mut warnings, &mut passwords);
    }
    Ok((nodes, passwords, warnings))
}

/* ------------------------------ tests ------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        "<?xml version='1.0'?>\n\
<KeePassFile>\n\
  <Root>\n\
    <Group>\n\
      <Name>Servers</Name>\n\
      <Entry>\n\
        <String><Key>Title</Key><Value>DB Prod</Value></String>\n\
        <String><Key>UserName</Key><Value>postgres</Value></String>\n\
        <String><Key>Password</Key><Value>s3cret</Value></String>\n\
        <String><Key>URL</Key><Value>10.0.0.50</Value></String>\n\
        <String><Key>Notes</Key><Value>primary replica</Value></String>\n\
        <History><Entry><String><Key>Title</Key><Value>old title</Value></String></Entry></History>\n\
      </Entry>\n\
      <Entry>\n\
        <String><Key>Title</Key><Value>WiFi Notes</Value></String>\n\
        <String><Key>UserName</Key><Value>guest</Value></String>\n\
        <String><Key>Password</Key><Value>wifi-pass</Value></String>\n\
      </Entry>\n\
      <Group>\n\
        <Name>Staging</Name>\n\
        <Entry>\n\
          <String><Key>Title</Key><Value>API</Value></String>\n\
          <String><Key>UserName</Key><Value>deploy</Value></String>\n\
          <String><Key>Password</Key><Value>deploy-pass</Value></String>\n\
          <String><Key>URL</Key><Value>staging.example</Value></String>\n\
        </Entry>\n\
      </Group>\n\
    </Group>\n\
  </Root>\n\
</KeePassFile>\n".to_string()
    }

    fn has_warning(warnings: &[String], needle: &str) -> bool {
        for w in warnings {
            if w.find(needle).is_some() {
                return true;
            }
        }
        false
    }

    #[test]
    fn maps_groups_and_entries() {
        let (nodes, passwords, warnings) = parse_xml(&sample()).unwrap();
        assert_eq!(nodes.len(), 1);
        let servers = &nodes[0];
        assert_eq!(servers.name, "Servers");
        assert_eq!(servers.children.len(), 2); // DB Prod + Staging folder

        let db = &servers.children[0];
        assert_eq!(db.name, "DB Prod");
        assert_eq!(db.hostname, "10.0.0.50");
        assert_eq!(db.username, "postgres");
        assert_eq!(db.port, "22"); // default

        let staging = &servers.children[1];
        assert_eq!(staging.name, "Staging");
        assert_eq!(staging.children.len(), 1);
        assert_eq!(staging.children[0].hostname, "staging.example");

        // passwords keyed by the same path convention as the frontend pathKey()
        assert_eq!(passwords.get("Servers/DB Prod"), Some(&"s3cret".to_string()));
        assert_eq!(passwords.get("Servers/Staging/API"), Some(&"deploy-pass".to_string()));

        // an entry without a URL is skipped (not imported, no password)
        assert!(has_warning(&warnings, "WiFi Notes"));
        assert!(passwords.get("Servers/WiFi Notes").is_none());
    }
}