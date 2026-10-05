import { useCallback, useEffect, useRef, useState } from "react";
import {
  homeDir,
  localList,
  localMkdir,
  localRemove,
  sftpClose,
  sftpList,
  sftpOpen,
  sftpOp,
  type LocalEntry,
  type SftpEntry,
  type SftpOp,
} from "../api";
import type { Tab } from "../types";
import type { ToastKind } from "../hooks/useToasts";

/**
 * WinSCP-style dual-pane SFTP commander, bound to one persistent `sftp` child
 * (tab id = session id). Sequential scripted transfers (v0.1); byte-level
 * progress / parallelism land with the russh driver (Phase 6).
 *
 * Keybindings (WinSSHTerm parity): F5 copy, F6 move, F7 mkdir, F8 delete,
 * F9 properties, Ctrl+U swap panes, Ctrl+R refresh, Ctrl+T terminal to host.
 */

type Side = "local" | "remote";

interface Props {
  tab: Tab;
  active: boolean;
  onToast: (msg: string, kind?: ToastKind) => void;
  /** Ctrl+T in the commander opens a terminal tab for the same host. */
  onOpenTerminal: (tab: Tab) => void;
}

function joinPath(dir: string, name: string): string {
  if (dir === "/") return `/${name}`;
  return `${dir.replace(/\/+$/, "")}/${name}`;
}

function parentPath(p: string): string {
  const t = p.replace(/\/+$/, "");
  const i = t.lastIndexOf("/");
  if (i <= 0) return "/";
  return t.slice(0, i);
}

function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let v = n;
  let u = -1;
  do {
    v /= 1024;
    u++;
  } while (v >= 1024 && u < units.length - 1);
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[u]}`;
}

function sortEntries<T extends { is_dir: boolean; name: string }>(a: T, b: T): number {
  if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
  return a.name.localeCompare(b.name);
}

export function CommanderView({ tab, active, onToast, onOpenTerminal }: Props) {
  const [remoteCwd, setRemoteCwd] = useState<string | null>(null);
  const [localCwd, setLocalCwd] = useState<string>("");
  const [remoteEntries, setRemoteEntries] = useState<SftpEntry[]>([]);
  const [localEntries, setLocalEntries] = useState<LocalEntry[]>([]);
  const [sel, setSel] = useState<{ local: string | null; remote: string | null }>({
    local: null,
    remote: null,
  });
  const [focus, setFocus] = useState<Side>("remote");
  const [busy, setBusy] = useState(false);
  const [busyMsg, setBusyMsg] = useState<string | null>(null);
  const [status, setStatus] = useState("Connected — ready");
  const [fatal, setFatal] = useState<string | null>(null);
  const [props, setProps] = useState<{ side: Side; name: string; path: string; perms: string; size: number; owner: string; group: string; mtime: string; is_dir: boolean; is_link: boolean } | null>(null);

  const mounted = useRef(true);
  const opened = useRef(false);

  const refreshRemote = useCallback(
    async (path: string) => {
      if (!mounted.current) return;
      try {
        const list = await sftpList(tab.id, path);
        const clean = list.filter((e) => e.name !== "." && e.name !== "..");
        clean.sort(sortEntries);
        setRemoteEntries(clean);
      } catch (e) {
        onToast(`Remote listing failed: ${String(e)}`, "error");
      }
    },
    [tab.id, onToast],
  );

  const refreshLocal = useCallback(
    async (path: string) => {
      if (!mounted.current) return;
      try {
        const list = await localList(path);
        list.sort(sortEntries);
        setLocalEntries(list);
      } catch (e) {
        onToast(`Local listing failed: ${String(e)}`, "error");
      }
    },
    [onToast],
  );

  const refreshBoth = useCallback(
    async (r: string | null, l: string) => {
      if (r) void refreshRemote(r);
      void refreshLocal(l);
    },
    [refreshRemote, refreshLocal],
  );

  // Open the persistent sftp child once per tab.
  useEffect(() => {
    if (opened.current) return;
    opened.current = true;
    let cancelled = false;
    (async () => {
      try {
        const [cwd, home] = await Promise.all([sftpOpen(tab.id, tab.spec, tab.password), homeDir()]);
        if (cancelled) return;
        setRemoteCwd(cwd);
        setLocalCwd(home);
        setStatus(`${tab.target} — SFTP connected`);
        void refreshRemote(cwd);
        void refreshLocal(home);
      } catch (e) {
        if (!cancelled) {
          setFatal(String(e));
          setStatus("SFTP connection failed");
        }
      }
    })();
    return () => {
      cancelled = true;
      mounted.current = false;
      void sftpClose(tab.id).catch(() => {});
    };
    // tab.id is the session identity; spec changes must not respawn
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.id]);

  // Re-fetch listings when the tab becomes visible again.
  useEffect(() => {
    if (active && remoteCwd && localCwd) void refreshBoth(remoteCwd, localCwd);
  }, [active, remoteCwd, localCwd, refreshBoth]);

  const run = useCallback(
    async (msg: string, fn: () => Promise<unknown>) => {
      setBusy(true);
      setBusyMsg(msg);
      setStatus(msg);
      try {
        await fn();
        return true;
      } catch (e) {
        onToast(String(e), "error");
        setStatus(`Failed: ${String(e)}`);
        return false;
      } finally {
        setBusy(false);
        setBusyMsg(null);
      }
    },
    [onToast],
  );

  const cd = useCallback(
    (side: Side, path: string) => {
      if (side === "remote") {
        setRemoteCwd(path);
        void refreshRemote(path);
      } else {
        setLocalCwd(path);
        void refreshLocal(path);
      }
    },
    [refreshRemote, refreshLocal],
  );

  const selectedEntry = useCallback(
    (side: Side) => {
      if (side === "remote") {
        const n = sel.remote;
        return n ? remoteEntries.find((e) => e.name === n) ?? null : null;
      }
      const n = sel.local;
      return n ? localEntries.find((e) => e.name === n) ?? null : null;
    },
    [sel, remoteEntries, localEntries],
  );

  const copySelection = useCallback(async () => {
    const src = focus;
    const entry = selectedEntry(src);
    if (!entry) {
      onToast("Select an item in the focused pane first", "error");
      return;
    }
    if (src === "remote") {
      const target = joinPath(localCwd, entry.name);
      const ok = await run(`Downloading ${entry.name}…`, () =>
        sftpOp(tab.id, { op: "get", remote: (entry as SftpEntry).path, local: target }, 600),
      );
      if (ok) {
        onToast(`Downloaded ${entry.name}`, "success");
        setStatus(`Downloaded ${entry.name} → ${target}`);
        void refreshBoth(remoteCwd, localCwd);
      }
    } else {
      const target = joinPath(remoteCwd ?? "/", entry.name);
      const ok = await run(`Uploading ${entry.name}…`, () =>
        sftpOp(tab.id, { op: "put", local: joinPath(localCwd, entry.name), remote: target }, 600),
      );
      if (ok) {
        onToast(`Uploaded ${entry.name}`, "success");
        setStatus(`Uploaded ${entry.name} → ${target}`);
        void refreshBoth(remoteCwd, localCwd);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, selectedEntry, localCwd, remoteCwd, tab.id, run, onToast, refreshBoth]);

  const moveSelection = useCallback(async () => {
    const src = focus;
    const entry = selectedEntry(src);
    if (!entry) {
      onToast("Select an item in the focused pane first", "error");
      return;
    }
    // WinSCP F6 = cross-pane move: copy then delete the source.
    const srcPath = joinPath(src === "remote" ? (remoteCwd ?? "/") : localCwd, entry.name);
    if (src === "remote") {
      const target = joinPath(localCwd, entry.name);
      const ok = await run(`Moving ${entry.name} → local…`, async () => {
        await sftpOp(tab.id, { op: "get", remote: (entry as SftpEntry).path, local: target }, 600);
        await sftpOp(tab.id, { op: "rm_r", path: (entry as SftpEntry).path, is_dir: entry.is_dir }, 120);
      });
      if (ok) {
        onToast(`Moved ${entry.name} to local`, "success");
        void refreshBoth(remoteCwd, localCwd);
      }
    } else {
      const target = joinPath(remoteCwd ?? "/", entry.name);
      const ok = await run(`Moving ${entry.name} → remote…`, async () => {
        await sftpOp(tab.id, { op: "put", local: srcPath, remote: target }, 600);
        await localRemove(srcPath, entry.is_dir);
      });
      if (ok) {
        onToast(`Moved ${entry.name} to remote`, "success");
        void refreshBoth(remoteCwd, localCwd);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, selectedEntry, localCwd, remoteCwd, tab.id, run, onToast, refreshBoth]);

  const makeDir = useCallback(async () => {
    const name = window.prompt(`Create folder in ${focus === "remote" ? remoteCwd : localCwd}:`);
    if (!name) return;
    const path = joinPath(focus === "remote" ? (remoteCwd ?? "/") : localCwd, name);
    if (focus === "remote") {
      const ok = await run(`Creating ${path}…`, () =>
        sftpOp(tab.id, { op: "mkdir", path }, 30),
      );
      if (ok) {
        onToast(`Created ${path}`, "success");
        if (remoteCwd) void refreshRemote(remoteCwd);
      }
    } else {
      const ok = await run(`Creating ${path}…`, () => localMkdir(path));
      if (ok) {
        onToast(`Created ${path}`, "success");
        void refreshLocal(localCwd);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, remoteCwd, localCwd, tab.id, run, onToast, refreshRemote, refreshLocal]);

  const deleteSelection = useCallback(async () => {
    const side = focus;
    const entry = selectedEntry(side);
    if (!entry) {
      onToast("Select an item in the focused pane first", "error");
      return;
    }
    const what = entry.is_dir ? "folder (recursively)" : "file";
    if (!window.confirm(`Delete ${what} "${entry.name}"?`)) return;
    const path = joinPath(side === "remote" ? (remoteCwd ?? "/") : localCwd, entry.name);
    if (side === "remote") {
      const ok = await run(`Deleting ${entry.name}…`, () =>
        sftpOp(tab.id, { op: "rm_r", path: (entry as SftpEntry).path, is_dir: entry.is_dir }, 120),
      );
      if (ok) {
        onToast(`Deleted ${entry.name}`, "success");
        if (remoteCwd) void refreshRemote(remoteCwd);
      }
    } else {
      const ok = await run(`Deleting ${entry.name}…`, () => localRemove(path, entry.is_dir));
      if (ok) {
        onToast(`Deleted ${entry.name}`, "success");
        void refreshLocal(localCwd);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, selectedEntry, remoteCwd, localCwd, tab.id, run, onToast, refreshRemote, refreshLocal]);

  const showProperties = useCallback(() => {
    const side = focus;
    const entry = selectedEntry(side);
    if (!entry) {
      onToast("Select an item in the focused pane first", "error");
      return;
    }
    setProps({
      side,
      name: entry.name,
      path: (side === "remote" ? (entry as SftpEntry).path : joinPath(localCwd, entry.name)),
      perms: entry.perms,
      size: entry.size,
      owner: side === "remote" ? (entry as SftpEntry).owner : "—",
      group: side === "remote" ? (entry as SftpEntry).group : "—",
      mtime: entry.mtime,
      is_dir: entry.is_dir,
      is_link: entry.is_link,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, selectedEntry, localCwd, onToast]);

  const openSelection = useCallback(() => {
    const side = focus;
    const entry = selectedEntry(side);
    if (!entry) return;
    if (side === "remote") {
      const e = entry as SftpEntry;
      if (e.is_dir) cd("remote", e.path);
    } else if (entry.is_dir) {
      cd("local", joinPath(localCwd, entry.name));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, selectedEntry, cd, localCwd]);

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const k = e.key;
      const ctrl = e.ctrlKey;
      // Don't hijack keys while the user edits a path bar.
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if (ctrl && k.toLowerCase() === "t") {
        e.preventDefault();
        e.stopPropagation();
        onOpenTerminal(tab);
      } else if (ctrl && k.toLowerCase() === "u") {
        e.preventDefault();
        e.stopPropagation();
        setFocus((f) => (f === "remote" ? "local" : "remote"));
      } else if (ctrl && k.toLowerCase() === "r") {
        e.preventDefault();
        e.stopPropagation();
        void refreshBoth(remoteCwd, localCwd);
      } else if (k === "F5") {
        e.preventDefault();
        e.stopPropagation();
        void copySelection();
      } else if (k === "F6") {
        e.preventDefault();
        e.stopPropagation();
        void moveSelection();
      } else if (k === "F7") {
        e.preventDefault();
        e.stopPropagation();
        void makeDir();
      } else if (k === "F8") {
        e.preventDefault();
        e.stopPropagation();
        void deleteSelection();
      } else if (k === "F9") {
        e.preventDefault();
        e.stopPropagation();
        showProperties();
      } else if (k === "Enter") {
        e.preventDefault();
        openSelection();
      } else if (k === "Backspace") {
        e.preventDefault();
        cd(focus, focus === "remote" ? parentPath(remoteCwd ?? "/") : parentPath(localCwd));
      }
    },
    [onOpenTerminal, tab, refreshBoth, remoteCwd, localCwd, copySelection, moveSelection, makeDir, deleteSelection, showProperties, openSelection, cd, focus],
  );

  // When this commander becomes the active tab, grab keyboard focus.
  const rootRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (active) rootRef.current?.focus();
  }, [active]);

  if (fatal) {
    return (
      <div className="term-pane commander">
        <div className="cmd-fatal">
          <h3>SFTP connection failed</h3>
          <pre>{fatal}</pre>
          <p className="muted">Check host / user / key (and known_hosts) in the Configuration panel, then close the tab.</p>
        </div>
      </div>
    );
  }

  const pane = (side: Side) => {
    const cwd = side === "remote" ? (remoteCwd ?? "/") : localCwd || "/";
    const activeFocus = focus === side;
    const hasParent = cwd !== "/";
    return (
      <div className={`cmd-pane${activeFocus ? " focused" : ""}`} onMouseDown={() => setFocus(side)}>
        <div className="cmd-pane-title">
          <span>{side === "remote" ? `Remote — ${tab.target}` : "Local"}</span>
        </div>
        <div className="cmd-pathbar">
          <button className="tbtn" title="Up" disabled={!hasParent} onClick={() => cd(side, parentPath(cwd))}>
            ⬆
          </button>
          <input
            value={cwd}
            spellCheck={false}
            onChange={(e) => {
              if (side === "remote") setRemoteCwd(e.target.value);
              else setLocalCwd(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                const v = (e.target as HTMLInputElement).value;
                cd(side, v);
              }
            }}
          />
          <button className="tbtn" title="Refresh (Ctrl+R)" onClick={() => (side === "remote" ? refreshRemote(cwd) : refreshLocal(cwd))}>
            ⟳
          </button>
        </div>
        <div className="cmd-head">
          <span className="c-name">Name</span>
          <span className="c-size">Size</span>
          <span className="c-mtime">Modified / Perms</span>
        </div>
        <div className="cmd-list">
          {rowsFor(side, cwd).map((row) => (
            <div
              key={row.key}
              className={`cmd-row${row.kind === "up" ? " up" : ""}${(!row.entry && row.kind !== "up") || (sel[side] === row.key && row.kind !== "up") ? " selected" : ""}`}
              onMouseDown={(ev) => {
                ev.stopPropagation();
                setFocus(side);
                if (row.kind !== "up" && row.entry) {
                  setSel((s) => ({ ...s, [side]: row.key }));
                }
              }}
              onDoubleClick={() => {
                if (row.kind === "up") cd(side, parentPath(cwd));
                else if (row.entry && (row.entry as SftpEntry | LocalEntry).is_dir) {
                  if (side === "remote") cd("remote", (row.entry as SftpEntry).path);
                  else cd("local", joinPath(localCwd, (row.entry as LocalEntry).name));
                }
              }}
              title={row.perms}
            >
              <span className={`c-name icon-${row.kind}`}>{row.label}</span>
              <span className="c-size">{row.size}</span>
              <span className="c-mtime">{row.mtime}</span>
            </div>
          ))}
          {rowsFor(side, cwd).length === 0 && <div className="cmd-empty">(empty)</div>}
        </div>
        <div className="cmd-toolbar">
          <button className="tbtn" title="New folder (F7)" onClick={() => { setFocus(side); void makeDir(); }}>📁+</button>
          <button className="tbtn" title="Delete (F8)" onClick={() => { setFocus(side); void deleteSelection(); }}>🗑</button>
          <button className="tbtn" title="Properties (F9)" onClick={() => { setFocus(side); showProperties(); }}>ℹ</button>
          {side === "remote" && (
            <span className="cmd-remote-hint">
              F5 copy · F6 move · F7 mkdir · F8 del · F9 props · Ctrl+U swap · Ctrl+R refresh
            </span>
          )}
        </div>
      </div>
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  };

  const rowsFor = (side: Side, cwd: string): Row[] => {
    const rows: Row[] = [];
    if (cwd !== "/") {
      rows.push({ key: "(up)", label: "..", kind: "up", size: "", mtime: "", perms: "", entry: null });
    }
    const items: Row[] =
      side === "remote"
        ? remoteEntries.map((e): Row => ({ key: e.path, label: e.name, kind: e.is_dir ? "dir" : e.is_link ? "link" : "file", size: e.is_dir ? "" : humanSize(e.size), mtime: e.mtime, perms: e.perms, entry: e }))
        : localEntries.map((e): Row => ({ key: e.name, label: e.name, kind: e.is_dir ? "dir" : e.is_link ? "link" : "file", size: e.is_dir ? "" : humanSize(e.size), mtime: e.mtime, perms: e.perms, entry: e }));
    return rows.concat(items);
  };

  type Row = { key: string; label: string; kind: "up" | "dir" | "file" | "link"; size: string; mtime: string; perms: string; entry: SftpEntry | LocalEntry | null };

  return (
    <div className={`term-pane commander${active ? "" : " hidden"}`} tabIndex={0} ref={rootRef} onKeyDown={onKeyDown}>
      <div className="cmd-body">
        {pane("local")}
        {pane("remote")}
      </div>
      <div className="cmd-progress-strip">
        <span className={`cmd-progress-dot${busy ? " active" : ""}`} />
        <span className="cmd-progress-text">{busyMsg ?? status}</span>
      </div>

      {props && (
        <div className="modal-backdrop" onMouseDown={() => setProps(null)}>
          <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
            <h3>Properties — {props.name}</h3>
            <div className="modal-body prop-grid">
              <div><b>Type</b><span>{props.is_dir ? "Folder" : props.is_link ? "Symbolic link" : "File"}</span></div>
              <div><b>Path</b><span className="mono">{props.path}</span></div>
              <div><b>Permissions</b><span className="mono">{props.perms}</span></div>
              <div><b>Size</b><span>{props.size > 0 ? `${humanSize(props.size)} (${props.size} bytes)` : "—"}</span></div>
              <div><b>Owner:Group</b><span>{props.owner}:{props.group}</span></div>
              <div><b>Modified</b><span>{props.mtime}</span></div>
            </div>
            <div className="modal-actions">
              {props.side === "remote" && (
                <button
                  className="btn"
                  onClick={() => {
                    const mode = window.prompt("chmod — numeric mode (e.g. 755):", "755");
                    if (!mode) return;
                    void run(`chmod ${mode} ${props.name}…`, () =>
                      sftpOp(tab.id, { op: "chmod", mode, path: props.path } as SftpOp, 30),
                    ).then((ok) => {
                      if (ok && remoteCwd) {
                        void refreshRemote(remoteCwd);
                        onToast(`chmod ${mode} on ${props.name}`, "success");
                      }
                    });
                    setProps(null);
                  }}
                >
                  chmod…
                </button>
              )}
              <button className="btn primary" onClick={() => setProps(null)}>Close</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
