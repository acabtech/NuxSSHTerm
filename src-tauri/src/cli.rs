//! Headless command-line interface (issues #27, #26).
//!
//! Reuses the free-standing store/model/importcmd core (no Tauri state) so the
//! installed binary can perform non-interactive, script/LLM-friendly
//! operations before the GUI starts. Conventions:
//!
//! - Data goes to stdout; diagnostics and warnings go to stderr.
//! - `--json` switches data commands to stable, machine-readable JSON.
//! - Exit codes: 0 success, 1 runtime error, 2 usage error.
//! - No prompts: missing prerequisites fail fast with a clear error.
//! - Stable identifiers: sessions are addressed by their `/`-joined tree path
//!   (the same key scheme the vault and settings use).
//! - Secrets are never printed: vault contents stay locked headless and
//!   exports always strip passwords.

use crate::importcmd;
use crate::model::{self, KIND_CONNECTION, Node};
use crate::store;
use serde_json::json;

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

/// A parsed CLI invocation.
pub struct Invocation {
    pub json: bool,
    pub command: Command,
}

pub enum Command {
    List,
    Status,
    Paths,
    Import { path: String, commit: bool },
    Export { path: String, force: bool },
    ApiKeys(ApiKeysAction),
    /// Opt-in integrations / automation (issues #22, #23).
    Vault(VaultAction),
    Omarchy(OmarchyAction),
    Help,
    Version,
}

/// Vault automation: verify the configured API key without printing secrets.
pub enum VaultAction {
    Status,
    Unlock,
}

/// Omarchy menu publisher controls.
pub enum OmarchyAction {
    Status,
    Sync,
    Disable,
}

/// `apikeys` subcommand (issue #25).
pub enum ApiKeysAction {
    /// Generate a new key for `name`; prints the plaintext exactly once.
    Create { name: String },
    /// Print metadata (id/name/created) for all keys — never the plaintext.
    List,
    /// Revoke the key by id.
    Revoke { id: String },
}

pub enum ParseResult {
    Cli(Invocation),
    /// Not a CLI invocation — start the GUI. `session` is the stable path key
    /// written by Omarchy menu rows (`--session <path>`).
    Gui { session: Option<String> },
    UsageError {
        message: String,
        json: bool,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum Sub {
    List,
    Status,
    Paths,
    Import,
    Export,
    ApiKeys,
    ApiKeysCreate,
    ApiKeysList,
    ApiKeysRevoke,
    Vault,
    VaultStatus,
    VaultUnlock,
    Omarchy,
    OmarchyStatus,
    OmarchySync,
    OmarchyDisable,
}

/// Parse CLI arguments (argv[1..]). Unknown leading tokens fall back to the
/// GUI so desktop launches with stray arguments keep working; once a
/// recognized subcommand is seen, anything unrecognized is a usage error.
pub fn parse(args: &[String]) -> ParseResult {
    let mut json = false;
    let mut sub: Option<Sub> = None;
    let mut path: Option<String> = None;
    let mut commit = false;
    let mut force = false;
    let mut apikeys_verb: Option<Sub> = None;
    let mut vault_verb: Option<Sub> = None;
    let mut omarchy_verb: Option<Sub> = None;
    let mut session: Option<String> = None;
    let mut awaiting_session = false;

    for a in args {
        if awaiting_session {
            session = Some(a.to_string());
            awaiting_session = false;
            continue;
        }
        match a.as_str() {
            "--json" => json = true,
            "--session" if sub.is_none() => awaiting_session = true,
            "--session" => {
                return ParseResult::UsageError {
                    message: "--session only applies when starting the GUI".into(),
                    json,
                };
            }
            "-h" | "--help" => {
                return ParseResult::Cli(Invocation {
                    json,
                    command: Command::Help,
                });
            }
            "-V" | "--version" => {
                return ParseResult::Cli(Invocation {
                    json,
                    command: Command::Version,
                });
            }
            "help" if sub.is_none() => {
                return ParseResult::Cli(Invocation {
                    json,
                    command: Command::Help,
                });
            }
            "version" if sub.is_none() => {
                return ParseResult::Cli(Invocation {
                    json,
                    command: Command::Version,
                });
            }
            "list" if sub.is_none() => sub = Some(Sub::List),
            "status" if sub.is_none() => sub = Some(Sub::Status),
            "paths" if sub.is_none() => sub = Some(Sub::Paths),
            "import" if sub.is_none() => sub = Some(Sub::Import),
            "export" if sub.is_none() => sub = Some(Sub::Export),
            "--yes" if sub == Some(Sub::Import) => commit = true,
            "--force" if sub == Some(Sub::Export) => force = true,
            "apikeys" if sub.is_none() => sub = Some(Sub::ApiKeys),
            "vault" if sub.is_none() => sub = Some(Sub::Vault),
            "status" if sub == Some(Sub::Vault) && vault_verb.is_none() => {
                vault_verb = Some(Sub::VaultStatus)
            }
            "unlock" if sub == Some(Sub::Vault) && vault_verb.is_none() => {
                vault_verb = Some(Sub::VaultUnlock)
            }
            "omarchy" if sub.is_none() => sub = Some(Sub::Omarchy),
            "status" if sub == Some(Sub::Omarchy) && omarchy_verb.is_none() => {
                omarchy_verb = Some(Sub::OmarchyStatus)
            }
            "sync" if sub == Some(Sub::Omarchy) && omarchy_verb.is_none() => {
                omarchy_verb = Some(Sub::OmarchySync)
            }
            "disable" if sub == Some(Sub::Omarchy) && omarchy_verb.is_none() => {
                omarchy_verb = Some(Sub::OmarchyDisable)
            }
            "create" if sub == Some(Sub::ApiKeys) && apikeys_verb.is_none() => {
                apikeys_verb = Some(Sub::ApiKeysCreate)
            }
            "list" if sub == Some(Sub::ApiKeys) && apikeys_verb.is_none() => {
                apikeys_verb = Some(Sub::ApiKeysList)
            }
            "revoke" if sub == Some(Sub::ApiKeys) && apikeys_verb.is_none() => {
                apikeys_verb = Some(Sub::ApiKeysRevoke)
            }
            _ if a.starts_with('-') && sub.is_some() => {
                return ParseResult::UsageError {
                    message: format!("unknown option: {a}"),
                    json,
                };
            }
            _ if sub.is_none() => return ParseResult::Gui { session: None },
            _ if sub == Some(Sub::List) || sub == Some(Sub::Status) || sub == Some(Sub::Paths) => {
                return ParseResult::UsageError {
                    message: format!("unexpected argument: {a}"),
                    json,
                };
            }
            // apikeys create/revoke take exactly one positional argument.
            _ if matches!(
                apikeys_verb,
                Some(Sub::ApiKeysCreate) | Some(Sub::ApiKeysRevoke)
            ) && path.is_none() =>
            {
                path = Some(a.to_string())
            }
            _ if sub == Some(Sub::ApiKeys) => {
                return ParseResult::UsageError {
                    message: format!("unexpected argument: {a}"),
                    json,
                };
            }
            _ if matches!(sub, Some(Sub::Vault) | Some(Sub::Omarchy)) => {
                return ParseResult::UsageError {
                    message: format!("unexpected argument: {a}"),
                    json,
                };
            }
            _ if path.is_none() => path = Some(a.to_string()),
            _ => {
                return ParseResult::UsageError {
                    message: format!("unexpected argument: {a}"),
                    json,
                };
            }
        }
    }

    if awaiting_session {
        return ParseResult::UsageError {
            message: "--session requires a <path>".into(),
            json,
        };
    }
    if session.is_some() && sub.is_some() {
        return ParseResult::UsageError {
            message: "--session only applies when starting the GUI".into(),
            json,
        };
    }

    match sub {
        None => ParseResult::Gui { session }, // flags only → GUI (preserve current behavior)
        Some(Sub::List) => ParseResult::Cli(Invocation {
            json,
            command: Command::List,
        }),
        Some(Sub::Status) => ParseResult::Cli(Invocation {
            json,
            command: Command::Status,
        }),
        Some(Sub::Paths) => ParseResult::Cli(Invocation {
            json,
            command: Command::Paths,
        }),
        Some(Sub::ApiKeys) => match (apikeys_verb, path) {
            (Some(Sub::ApiKeysList), _) => ParseResult::Cli(Invocation {
                json,
                command: Command::ApiKeys(ApiKeysAction::List),
            }),
            (Some(Sub::ApiKeysCreate), Some(name)) => ParseResult::Cli(Invocation {
                json,
                command: Command::ApiKeys(ApiKeysAction::Create { name }),
            }),
            (Some(Sub::ApiKeysRevoke), Some(id)) => ParseResult::Cli(Invocation {
                json,
                command: Command::ApiKeys(ApiKeysAction::Revoke { id }),
            }),
            (Some(Sub::ApiKeysCreate), None) => ParseResult::UsageError {
                message: "apikeys create requires a <name>".into(),
                json,
            },
            (Some(Sub::ApiKeysRevoke), None) => ParseResult::UsageError {
                message: "apikeys revoke requires a key <id>".into(),
                json,
            },
            _ => ParseResult::UsageError {
                message: "apikeys requires create|list|revoke".into(),
                json,
            },
        },
        Some(Sub::Vault) => match vault_verb {
            Some(Sub::VaultStatus) => ParseResult::Cli(Invocation {
                json,
                command: Command::Vault(VaultAction::Status),
            }),
            Some(Sub::VaultUnlock) => ParseResult::Cli(Invocation {
                json,
                command: Command::Vault(VaultAction::Unlock),
            }),
            None => ParseResult::UsageError {
                message: "vault requires status|unlock".into(),
                json,
            },
            _ => unreachable!("vault verbs handled above"),
        },
        Some(Sub::Omarchy) => match omarchy_verb {
            Some(Sub::OmarchyStatus) => ParseResult::Cli(Invocation {
                json,
                command: Command::Omarchy(OmarchyAction::Status),
            }),
            Some(Sub::OmarchySync) => ParseResult::Cli(Invocation {
                json,
                command: Command::Omarchy(OmarchyAction::Sync),
            }),
            Some(Sub::OmarchyDisable) => ParseResult::Cli(Invocation {
                json,
                command: Command::Omarchy(OmarchyAction::Disable),
            }),
            None => ParseResult::UsageError {
                message: "omarchy requires status|sync|disable".into(),
                json,
            },
            _ => unreachable!("omarchy verbs handled above"),
        },
        Some(Sub::Import) => match path {
            Some(path) => ParseResult::Cli(Invocation {
                json,
                command: Command::Import { path, commit },
            }),
            None => ParseResult::UsageError {
                message: "import requires a file path".into(),
                json,
            },
        },
        Some(Sub::Export) => match path {
            Some(path) => ParseResult::Cli(Invocation {
                json,
                command: Command::Export { path, force },
            }),
            None => ParseResult::UsageError {
                message: "export requires a file path".into(),
                json,
            },
        },
        // Sub-verbs are matched inside their parent arms above.
        _ => unreachable!("sub-verbs handled by their parent arms"),
    }
}

/// Print a CLI error: JSON error object on stderr in machine mode, `error: …`
/// in human mode. Never prints to stdout so data stays parseable.
pub fn print_error(json: bool, message: &str) {
    if json {
        eprintln!("{}", json!({ "error": message }));
    } else {
        eprintln!("error: {message}");
    }
}

fn print_warning(json: bool, message: &str) {
    if json {
        eprintln!("{}", json!({ "warning": message }));
    } else {
        eprintln!("warning: {message}");
    }
}

fn count_connections(nodes: &[Node]) -> usize {
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    model::flatten(nodes, &empty, &mut flat);
    flat.iter()
        .filter(|(_, n)| n.node_type == KIND_CONNECTION)
        .count()
}

/// Execute a parsed invocation and return the process exit code.
pub fn execute(inv: &Invocation) -> i32 {
    match &inv.command {
        Command::Help => {
            print_help();
            EXIT_OK
        }
        Command::Version => {
            println!("nuxsshterm {}", env!("CARGO_PKG_VERSION"));
            EXIT_OK
        }
        Command::List => cmd_list(inv.json),
        Command::Status => cmd_status(inv.json),
        Command::Paths => cmd_paths(inv.json),
        Command::Import { path, commit } => cmd_import(path, *commit, inv.json),
        Command::Export { path, force } => cmd_export(path, *force, inv.json),
        Command::ApiKeys(action) => match action {
            ApiKeysAction::List => cmd_apikeys_list(inv.json),
            ApiKeysAction::Create { name } => cmd_apikeys_create(name, inv.json),
            ApiKeysAction::Revoke { id } => cmd_apikeys_revoke(id, inv.json),
        },
        Command::Vault(action) => cmd_vault(action, inv.json),
        Command::Omarchy(action) => cmd_omarchy(action, inv.json),
    }
}

fn cmd_apikeys_list(json: bool) -> i32 {
    let keys = store::api_keys();
    if json {
        let items: Vec<_> = keys
            .iter()
            .map(|k| json!({ "id": k.id, "name": k.name, "created": k.created }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
        );
    } else if keys.is_empty() {
        println!("no API keys — create one: nuxsshterm apikeys create <name>");
    } else {
        for k in &keys {
            println!("{}\t{}\tcreated {}", k.id, k.name, k.created);
        }
    }
    EXIT_OK
}

fn cmd_apikeys_create(name: &str, json: bool) -> i32 {
    let generated = match crate::apikeys::generate(name).and_then(|g| {
        store::api_key_add(g.record).map(|record| (g.plaintext, record))
    }) {
        Ok(ok) => ok,
        Err(e) => {
            print_error(json, &e);
            return EXIT_ERROR;
        }
    };
    let (plaintext, record) = generated;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": record.id,
                "name": record.name,
                "created": record.created,
                "key": plaintext,
                "warning": "store this key now — it is never shown again",
            }))
            .unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!("API key created: {} ({})", record.name, record.id);
        println!();
        println!("{plaintext}");
        println!();
        println!("Store this key now — it is never shown again.");
    }
    EXIT_OK
}

fn cmd_apikeys_revoke(id: &str, json: bool) -> i32 {
    match store::api_key_revoke(id).inspect(|removed| {
        if *removed {
            crate::vault::on_api_key_revoked(id);
        }
    }) {
        Ok(true) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "revoked": id })).unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!("revoked API key {id}");
            }
            EXIT_OK
        }
        Ok(false) => {
            print_error(json, &format!("no API key with id {id:?}"));
            EXIT_ERROR
        }
        Err(e) => {
            print_error(json, &e);
            EXIT_ERROR
        }
    }
}

fn cmd_vault(action: &VaultAction, json: bool) -> i32 {
    match action {
        VaultAction::Status => {
            let status = crate::vault::auto_unlock_status();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!(
                    "auto-unlock: {}",
                    if status.configured {
                        "configured"
                    } else {
                        "not configured"
                    }
                );
                if status.configured {
                    println!(
                        "key: {} ({})",
                        status.key_id,
                        if status.key_registered {
                            "registered"
                        } else {
                            "revoked — auto-unlock will fail"
                        }
                    );
                }
            }
            EXIT_OK
        }
        VaultAction::Unlock => match crate::vault::auto_unlock_headless() {
            Ok(id) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "unlocked": true,
                            "keyId": id,
                            "note": "credentials verified in memory only; nothing persisted",
                        }))
                        .unwrap_or_else(|_| "{}".into())
                    );
                } else {
                    println!("auto-unlock credentials valid ({id}); no secrets printed");
                }
                EXIT_OK
            }
            Err(e) => {
                print_error(json, &e);
                EXIT_ERROR
            }
        },
    }
}

fn cmd_omarchy(action: &OmarchyAction, json: bool) -> i32 {
    match action {
        OmarchyAction::Status => {
            let status = crate::omarchy::status();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!(
                    "Omarchy: {}",
                    if status.available { "detected" } else { "not detected" }
                );
                println!("menu: {}", status.menu_path);
                println!("published favorites: {}", status.published);
            }
            EXIT_OK
        }
        OmarchyAction::Sync => {
            if !crate::omarchy::available() {
                print_warning(
                    json,
                    "Omarchy config not found — nothing to do (feature is a no-op here)",
                );
            }
            match crate::omarchy::sync(&store::load_tree()) {
                Ok(count) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({ "published": count }))
                                .unwrap_or_else(|_| "{}".into())
                        );
                    } else {
                        println!("published {count} favorite session(s) to the Omarchy menu");
                    }
                    EXIT_OK
                }
                Err(e) => {
                    print_error(json, &e);
                    EXIT_ERROR
                }
            }
        }
        OmarchyAction::Disable => {
            let mut ui = store::get_ui_settings();
            ui.omarchy_menu_integration = false;
            match store::set_ui_settings(&ui).and_then(|_| crate::omarchy::sync_for_settings(&ui)) {
                Ok(_) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({ "disabled": true }))
                                .unwrap_or_else(|_| "{}".into())
                        );
                    } else {
                        println!("Omarchy menu integration disabled; managed rows removed");
                    }
                    EXIT_OK
                }
                Err(e) => {
                    print_error(json, &e);
                    EXIT_ERROR
                }
            }
        }
    }
}

fn print_help() {
    print!(
        "NuxSSHTerm {} — headless CLI for session/config operations.\n\
Without a recognized subcommand the GUI starts, as before.\n\
\n\
USAGE:\n    \
nuxsshterm <COMMAND> [ARGS] [--json]\n\
\n\
COMMANDS:\n    \
list              Print the session tree flattened to stable tree paths\n    \
status            Show store presence and session counts\n    \
paths             Print configuration/data file paths\n    \
import <file>     Parse a sessions file (WinSSHTerm XML/backup, PuTTY .reg,\n    \
                  KiTTY .txt, KeePass .kdbx). Preview only unless --yes.\n        \
--yes         Commit the import: replaces the stored session tree\n    \
export <file>     Export the session tree as WinSSHTerm XML\n        \
--force       Overwrite an existing target file\n\
\n\
API KEYS (issue #25):\n    \
apikeys create <name>  Generate a new key; the plaintext is printed once\n    \
apikeys list           Print id/name/created metadata (no secrets)\n    \
apikeys revoke <id>    Revoke the key with this id\n\
\n\
AUTOMATION (issues #22/#23):\n    \
vault status       Show opt-in auto-unlock configuration (no secrets)\n    \
vault unlock       Verify the configured API key can unlock the vault\n    \
omarchy status     Show Omarchy detection and published favorite count\n    \
omarchy sync       Publish favorite sessions to Omarchy Remote Hosts\n    \
omarchy disable    Remove the managed Omarchy menu rows\n\
\n\
FLAGS:\n    \
--json            Machine-readable JSON output on stdout\n    \
--session <path>  Launch the GUI selecting this stable session path\n    \
-h, --help        Print this help\n    \
-V, --version     Print version\n\
\n\
EXIT CODES:\n    \
0  success\n    \
1  runtime error (I/O, parse failure, unmet prerequisite)\n    \
2  usage error (unknown option, missing argument)\n\
\n\
CONVENTIONS:\n    \
- Data goes to stdout; diagnostics and warnings go to stderr.\n    \
- No interactive prompts: missing prerequisites fail fast with a clear error.\n    \
- Secrets are never printed. `vault unlock` only verifies auto-unlock\n    \
  credentials in memory; exports always strip passwords. Import never stores\n    \
  passwords (use the GUI to fill the encrypted vault).\n\
\n\
EXAMPLES:\n    \
nuxsshterm list --json\n    \
nuxsshterm import /tmp/connections.xml           # preview only\n    \
nuxsshterm import /tmp/connections.xml --yes     # replace stored tree\n    \
nuxsshterm export /tmp/export.xml\n\
nuxsshterm apikeys create ci-bot\n\
nuxsshterm apikeys list --json\n\
nuxsshterm apikeys revoke 1a2b3c4d\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn cmd_list(json: bool) -> i32 {
    let tree = store::load_tree();
    let mut flat: Vec<(Vec<String>, Node)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    model::flatten(&tree, &empty, &mut flat);

    if json {
        let items: Vec<_> = flat
            .iter()
            .map(|(path, n)| {
                json!({
                    "path": path.join("/"),
                    "type": n.node_type,
                    "name": n.name,
                    "hostname": n.hostname,
                    "username": n.username,
                    "port": n.port,
                    "privateKey": n.private_key,
                    "forwardAgent": n.forward_agent,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
        );
    } else {
        for (path, n) in &flat {
            if n.node_type == KIND_CONNECTION {
                println!(
                    "{}\tconnection\t{}@{}:{}",
                    path.join("/"),
                    n.username,
                    n.hostname,
                    n.port
                );
            } else {
                println!("{}\tcontainer\t", path.join("/"));
            }
        }
    }
    EXIT_OK
}

fn cmd_status(json: bool) -> i32 {
    let tree = store::load_tree();
    let connections = count_connections(&tree);
    let vault_initialized = crate::vault::is_initialized();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "configDir": store::config_dir().to_string_lossy(),
                "connectionCount": connections,
                "vaultInitialized": vault_initialized,
                "connectionsXml": store::connections_path().exists(),
                "settingsJson": store::settings_path().exists(),
            }))
            .unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!("config dir: {}", store::config_dir().display());
        println!("sessions: {connections} connection(s)");
        println!(
            "vault: {}",
            if vault_initialized {
                "initialized (locked — CLI never reads vault secrets)"
            } else {
                "not initialized"
            }
        );
        println!("note: vault unlock, terminal sessions, SFTP and the agent manager need the GUI");
    }
    EXIT_OK
}

fn cmd_paths(json: bool) -> i32 {
    let config = store::config_dir();
    let entries = [
        ("configDir", config.clone()),
        ("connectionsXml", store::connections_path()),
        ("settingsJson", store::settings_path()),
        ("vault", crate::vault::vault_path()),
        ("log", crate::log::log_path()),
    ];

    if json {
        let items: Vec<_> = entries
            .iter()
            .map(|(name, path)| {
                json!({
                    "name": name,
                    "path": path.to_string_lossy(),
                    "exists": path.exists(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
        );
    } else {
        for (name, path) in &entries {
            let marker = if path.exists() { "" } else { "  (missing)" };
            println!("{name}: {}{marker}", path.display());
        }
    }
    EXIT_OK
}

fn cmd_import(path: &str, commit: bool, json: bool) -> i32 {
    let preview = match importcmd::import_sessions_file(path.to_string(), None) {
        Ok(p) => p,
        Err(e) => {
            print_error(json, &e);
            return EXIT_ERROR;
        }
    };

    for w in &preview.warnings {
        print_warning(json, w);
    }

    if !commit {
        if json {
            // Defense in depth: the preview nodes are already stripped, but
            // never let a password reach stdout regardless.
            let mut nodes = preview.nodes.clone();
            model::strip_passwords(&mut nodes);
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "format": preview.format,
                    "count": preview.count,
                    "warnings": preview.warnings,
                    "needsMasterPassword": preview.needs_master_password,
                    "encryptedPasswords": preview.encrypted_passwords,
                    "nodes": nodes,
                    "committed": false,
                }))
                .unwrap_or_else(|_| "{}".into())
            );
        } else {
            println!("format: {}", preview.format);
            println!("sessions: {}", preview.count);
            if preview.needs_master_password {
                println!(
                    "encrypted passwords: locked — the WinSSHTerm master password is required (GUI import wizard)"
                );
            }
            println!("preview only — re-run with --yes to replace the stored session tree");
        }
        return EXIT_OK;
    }

    if preview.needs_master_password {
        print_error(
            json,
            "refusing to import: the file contains encrypted passwords that need the WinSSHTerm master password (GUI import wizard only) — the stored tree was not changed",
        );
        return EXIT_ERROR;
    }

    match store::save_tree(&preview.nodes) {
        Ok(()) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "format": preview.format,
                        "imported": preview.count,
                        "warnings": preview.warnings,
                        "committed": true,
                    }))
                    .unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!(
                    "imported {} session(s) from {} — stored session tree replaced",
                    preview.count, path
                );
            }
            EXIT_OK
        }
        Err(e) => {
            print_error(json, &e);
            EXIT_ERROR
        }
    }
}

fn cmd_export(path: &str, force: bool, json: bool) -> i32 {
    let tree = store::load_tree();
    let connections = count_connections(&tree);
    let target = std::path::Path::new(path);

    if target.exists() && !force {
        print_error(
            json,
            "target file already exists — pass --force to overwrite",
        );
        return EXIT_ERROR;
    }

    // store::export_xml strips every password before writing (vault invariant).
    match std::fs::write(target, store::export_xml(&tree)) {
        Ok(()) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "file": path,
                        "exported": connections,
                        "passwordsStripped": true,
                    }))
                    .unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!("exported {connections} connection(s) to {path} (passwords stripped)");
            }
            EXIT_OK
        }
        Err(e) => {
            print_error(json, &format!("cannot write {path}: {e}"));
            EXIT_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|o| o.to_string()).collect()
    }

    #[test]
    fn no_args_or_unknown_leading_token_starts_gui() {
        assert!(matches!(parse(&[]), ParseResult::Gui { .. }));
        assert!(matches!(parse(&s(&["--funky-flag"])), ParseResult::Gui { .. }));
        assert!(matches!(parse(&s(&["somefile.xml"])), ParseResult::Gui { .. }));
    }

    #[test]
    fn flags_only_start_gui() {
        assert!(matches!(parse(&s(&["--json"])), ParseResult::Gui { .. }));
    }

    #[test]
    fn json_flag_is_positional_independent() {
        for args in [s(&["--json", "list"]), s(&["list", "--json"])] {
            match parse(&args) {
                ParseResult::Cli(inv) => {
                    assert!(inv.json);
                    assert!(matches!(inv.command, Command::List));
                }
                _ => panic!("expected a CLI invocation"),
            }
        }
    }

    #[test]
    fn import_requires_path_and_honors_yes() {
        assert!(matches!(
            parse(&s(&["import"])),
            ParseResult::UsageError { .. }
        ));
        match parse(&s(&["import", "file.xml", "--yes", "--json"])) {
            ParseResult::Cli(inv) => match inv.command {
                Command::Import { path, commit } => {
                    assert_eq!(path, "file.xml");
                    assert!(commit);
                }
                _ => panic!("expected Import"),
            },
            _ => panic!("expected a CLI invocation"),
        }
    }

    #[test]
    fn export_honors_force_and_requires_path() {
        assert!(matches!(
            parse(&s(&["export"])),
            ParseResult::UsageError { .. }
        ));
        match parse(&s(&["export", "/tmp/out.xml", "--force"])) {
            ParseResult::Cli(inv) => match inv.command {
                Command::Export { path, force } => {
                    assert_eq!(path, "/tmp/out.xml");
                    assert!(force);
                }
                _ => panic!("expected Export"),
            },
            _ => panic!("expected a CLI invocation"),
        }
    }

    #[test]
    fn help_and_version_win_anywhere() {
        assert!(matches!(
            parse(&s(&["--help"])),
            ParseResult::Cli(Invocation {
                command: Command::Help,
                ..
            })
        ));
        assert!(matches!(
            parse(&s(&["list", "--version"])),
            ParseResult::Cli(Invocation {
                command: Command::Version,
                ..
            })
        ));
    }

    #[test]
    fn unknown_option_after_subcommand_is_usage_error() {
        assert!(matches!(
            parse(&s(&["list", "--wat"])),
            ParseResult::UsageError { .. }
        ));
    }

    #[test]
    fn second_subcommand_is_usage_error() {
        assert!(matches!(
            parse(&s(&["list", "status"])),
            ParseResult::UsageError { .. }
        ));
    }

    #[test]
    fn wrong_subcommand_flag_is_usage_error() {
        assert!(matches!(
            parse(&s(&["list", "--yes"])),
            ParseResult::UsageError { .. }
        ));
    }

    #[test]
    fn apikeys_verb_is_required() {
        assert!(matches!(
            parse(&s(&["apikeys"])),
            ParseResult::UsageError { .. }
        ));
    }

    #[test]
    fn apikeys_create_and_revoke_take_exactly_one_argument() {
        match parse(&s(&["apikeys", "create", "ci"])) {
            ParseResult::Cli(inv) => {
                assert!(matches!(inv.command, Command::ApiKeys(ApiKeysAction::Create { .. })))
            }
            _ => panic!("expected apikeys create"),
        }
        match parse(&s(&["apikeys", "revoke", "1a2b3c4d", "--json"])) {
            ParseResult::Cli(inv) => {
                assert!(inv.json);
                assert!(matches!(inv.command, Command::ApiKeys(ApiKeysAction::Revoke { .. })))
            }
            _ => panic!("expected apikeys revoke"),
        }
        assert!(matches!(
            parse(&s(&["apikeys", "create"])),
            ParseResult::UsageError { .. }
        ));
        assert!(matches!(
            parse(&s(&["apikeys", "create", "a", "b"])),
            ParseResult::UsageError { .. }
        ));
    }

    #[test]
    fn apikeys_list_standalone() {
        match parse(&s(&["apikeys", "list", "--json"])) {
            ParseResult::Cli(inv) => {
                assert!(inv.json);
                assert!(matches!(inv.command, Command::ApiKeys(ApiKeysAction::List)))
            }
            _ => panic!("expected apikeys list"),
        }
    }

    #[test]
    fn session_flag_launches_gui_with_target() {
        match parse(&s(&["--session", "grp/Prod"])) {
            ParseResult::Gui { session } => assert_eq!(session.as_deref(), Some("grp/Prod")),
            _ => panic!("expected GUI launch with a session"),
        }
        assert!(matches!(
            parse(&s(&["--session"])),
            ParseResult::UsageError { .. }
        ));
        assert!(matches!(
            parse(&s(&["list", "--session", "x"])),
            ParseResult::UsageError { .. }
        ));
    }

    #[test]
    fn vault_and_omarchy_verbs_parse() {
        match parse(&s(&["vault", "unlock", "--json"])) {
            ParseResult::Cli(inv) => {
                assert!(inv.json);
                assert!(matches!(inv.command, Command::Vault(VaultAction::Unlock)))
            }
            _ => panic!("expected vault unlock"),
        }
        match parse(&s(&["omarchy", "sync"])) {
            ParseResult::Cli(inv) => {
                assert!(matches!(inv.command, Command::Omarchy(OmarchyAction::Sync)))
            }
            _ => panic!("expected omarchy sync"),
        }
        assert!(matches!(
            parse(&s(&["vault"])),
            ParseResult::UsageError { .. }
        ));
    }
}
