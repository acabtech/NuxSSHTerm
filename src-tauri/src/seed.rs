//! Seed session tree — reconstructed from Sam's WinSSHTerm screenshot (2026-10-04).
//!
//! This is a placeholder so the UI is realistic before the real import arrives.
//! Hosts whose address is not known from the screenshot are seeded with an empty
//! Hostname and will be filled in by the settings import.

use crate::model::Node;

fn c(name: &str, expanded: bool) -> Node {
    Node::container(name, expanded)
}

/// `Some((host, user, port))` for hosts we know; `None` for placeholders.
fn conn(name: &str, known: Option<(&str, &str, u16)>) -> Node {
    match known {
        Some((host, user, port)) => Node::connection(name, host, user, port),
        None => Node::connection(name, "", "", 22),
    }
}

pub fn default_tree() -> Vec<Node> {
    vec![
        c("Cod IoT Admin", false),
        c("Cod WMS Prod", false),
        c("Cod Postgres Prod", false),
        c("Beleaf", false),
        c("Daya Tani", false),
        c("Personal Machines", false),
        c("Proxmox Hosts", true).with_children(vec![
            conn("Elitedesk One Host Local", Some(("192.168.100.201", "root", 22))),
            conn("Elitedesk Two Host Local", Some(("192.168.100.202", "root", 22))),
            conn("ThinkCentre Three Host Local", Some(("192.168.100.203", "root", 22))),
            conn("Jakiro Four Local", Some(("192.168.100.205", "root", 22))),
            conn("Ryzen Five Host Local", Some(("192.168.100.204", "root", 22))),
            conn("opnSense Local", None),
        ]),
        c("Proxmox VMs", true).with_children(vec![
            conn("Kiots-Knots", None),
            conn("Home-Services", None),
            conn("More-Services", Some(("192.168.100.130", "sam", 22))),
            conn("Teleport Backend", None),
            conn("TC3-Odoo-IoT", None),
            conn("Hermes Agent Sam", None),
            conn("Global Postgres", None),
            conn("LLAMA Backend", Some(("192.168.100.132", "sam", 22))),
            conn("Hermes Agent John", None),
            conn("OpenCode Sam", Some(("192.168.100.126", "sam", 22))),
            conn("Zabbix Home", None),
            conn("NtopNG Home", None),
            conn("Postgres Global", None),
            conn("Multica-Agent", None),
            conn("TC3-Consumer-App", None),
            conn("TC3-Fandi", None),
        ]),
        conn("opnSense Local", None),
        conn("Debian WSL", None),
        conn("Grandstream", None),
        c("Cloud VMs", true).with_children(vec![conn("AWS CDA Trampoline", None)]),
    ]
}