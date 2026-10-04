import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MenuBar, type MenuDef } from "./components/MenuBar";
import { ConfigPanel } from "./components/ConfigPanel";
import { SessionTree, type TreeCallbacks } from "./components/SessionTree";
import { StatusBar } from "./components/StatusBar";
import { TabStrip } from "./components/TabStrip";
import { TerminalView } from "./components/TerminalView";
import {
  exportConnectionsFile,
  getConfigDir,
  importConnectionsFile,
  loadTree,
  ptyWrite,
  saveTree,
} from "./api";
import { emptyNode, specFromNode, targetOf, type SessionNode, type Tab } from "./types";

/* ----------------------------- tree helpers ----------------------------- */

function getAt(tree: SessionNode[], path: number[]): SessionNode | null {
  let nodes = tree;
  let node: SessionNode | null = null;
  for (const i of path) {
    node = nodes[i] ?? null;
    if (!node) return null;
    nodes = node.children;
  }
  return node;
}

function updateAt(tree: SessionNode[], path: number[], patch: Partial<SessionNode>): SessionNode[] {
  if (path.length === 0) return tree;
  const [i, ...rest] = path;
  return tree.map((n, idx) => {
    if (idx !== i) return n;
    return rest.length === 0 ? { ...n, ...patch } : { ...n, children: updateAt(n.children, rest, patch) };
  });
}

function insertAt(tree: SessionNode[], parentPath: number[], node: SessionNode): SessionNode[] {
  if (parentPath.length === 0) return [...tree, node];
  const [i, ...rest] = parentPath;
  return tree.map((n, idx) =>
    idx !== i ? n : { ...n, expanded: true, children: insertAt(n.children, rest, node) },
  );
}

function removeAt(tree: SessionNode[], path: number[]): SessionNode[] {
  if (path.length === 0) return tree;
  const [i, ...rest] = path;
  if (rest.length === 0) return tree.filter((_, idx) => idx !== i);
  return tree.map((n, idx) => (idx !== i ? n : { ...n, children: removeAt(n.children, rest) }));
}

function countConnections(tree: SessionNode[]): number {
  let n = 0;
  const walk = (nodes: SessionNode[]) => {
    for (const x of nodes) {
      if (x.type === "Connection") n += 1;
      if (x.children.length) walk(x.children);
    }
  };
  walk(tree);
  return n;
}

function mapAll(tree: SessionNode[], fn: (n: SessionNode) => SessionNode): SessionNode[] {
  return tree.map((n) => fn({ ...n, children: n.children.length ? mapAll(n.children, fn) : [] }));
}

function uid(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

/* --------------------------------- app --------------------------------- */

type Modal =
  | { kind: "path"; action: "import" | "export"; title: string; value: string }
  | { kind: "info"; title: string; message: string }
  | null;

export default function App() {
  const [tree, setTree] = useState<SessionNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<number[] | null>(null);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTab, setActiveTab] = useState<string | null>(null);
  const [configDir, setConfigDir] = useState("");
  const [notice, setNotice] = useState("Loaded session tree");
  const [ctx, setCtx] = useState<{ x: number; y: number; path: number[] } | null>(null);
  const [tabCtx, setTabCtx] = useState<{ x: number; y: number; id: string } | null>(null);
  const [modal, setModal] = useState<Modal>(null);
  const treeLoaded = useRef(false);

  // initial load
  useEffect(() => {
    (async () => {
      try {
        const [t, dir] = await Promise.all([loadTree(), getConfigDir()]);
        setTree(t);
        setConfigDir(dir);
        treeLoaded.current = true;
        setNotice(`Loaded ${t.length} top-level entries`);
      } catch (e) {
        setNotice(`Load failed: ${String(e)}`);
      }
    })();
  }, []);

  // persist (skip the initial load round-trip)
  useEffect(() => {
    if (!treeLoaded.current) return;
    const h = window.setTimeout(() => {
      void saveTree(tree).catch((e) => setNotice(`Save failed: ${String(e)}`));
    }, 400);
    return () => window.clearTimeout(h);
  }, [tree]);

  const selected = useMemo(
    () => (selectedPath ? getAt(tree, selectedPath) : null),
    [tree, selectedPath],
  );

  const connCount = useMemo(() => countConnections(tree), [tree]);

  /* ------------------------------ sessions ------------------------------ */

  const openSession = useCallback(
    (path: number[]) => {
      const node = getAt(tree, path);
      if (!node || node.type === "Container") return;
      if (!node.hostname) {
        setNotice(`"${node.name}" has no Host/IP configured`);
        setModal({
          kind: "info",
          title: "No host configured",
          message:
            `"${node.name}" has no Host/IP yet.\n\n` +
            `This entry came from the rebuilt tree placeholder. Set Host/IP in the ` +
            `Configuration panel, or import your real WinSSHTerm settings via ` +
            `File → Import connections…`,
        });
        return;
      }
      const tab: Tab = {
        id: uid(),
        title: node.name,
        kind: "terminal",
        spec: specFromNode(node),
        target: targetOf(node),
        exited: false,
      };
      setTabs((t) => [...t, tab]);
      setActiveTab(tab.id);
      setNotice(`Opening ${tab.target}`);
    },
    [tree],
  );

  const openLocalShell = useCallback(() => {
    const tab: Tab = {
      id: uid(),
      title: "Local shell",
      kind: "terminal",
      spec: specFromNode(emptyNode("Local shell", "Connection")),
      target: `${navigator.platform || "local"} shell`,
      exited: false,
    };
    setTabs((t) => [...t, tab]);
    setActiveTab(tab.id);
  }, []);

  const closeTab = useCallback(
    (id: string) => {
      setTabs((prev) => {
        const idx = prev.findIndex((t) => t.id === id);
        const next = prev.filter((t) => t.id !== id);
        setActiveTab((cur) => {
          if (cur !== id) return cur;
          const fallback = next[Math.min(idx, next.length - 1)];
          return fallback ? fallback.id : null;
        });
        return next;
      });
    },
    [],
  );

  const closeOthers = useCallback((id: string) => {
    setTabs((prev) => prev.filter((t) => t.id === id));
    setActiveTab(id);
  }, []);

  const closeAllTabs = useCallback(() => {
    setTabs([]);
    setActiveTab(null);
  }, []);

  const currentTab = tabs.find((t) => t.id === activeTab) ?? null;

  const pasteIntoActive = useCallback(async () => {
    if (!currentTab) {
      setNotice("No active terminal to paste into");
      return;
    }
    try {
      const text = await navigator.clipboard.readText();
      if (text) await ptyWrite(currentTab.id, text);
      setNotice(`Pasted ${text.length} chars`);
    } catch (e) {
      setNotice(`Clipboard unavailable: ${String(e)}`);
    }
  }, [currentTab]);

  /* ------------------------------- actions ------------------------------ */

  const addSession = useCallback(
    (asContainer: boolean) => {
      const parentPath =
        selectedPath && selected?.type === "Container" ? selectedPath : [];
      const node = emptyNode(asContainer ? "New folder" : "New session", asContainer ? "Container" : "Connection");
      setTree((t) => insertAt(t, parentPath, node));
      setNotice(asContainer ? "Folder added" : "Session added");
    },
    [selected, selectedPath],
  );

  const duplicateSelected = useCallback(() => {
    if (!selectedPath) return;
    const node = getAt(tree, selectedPath);
    if (!node) return;
    const copy: SessionNode = JSON.parse(JSON.stringify(node));
    copy.name = `${node.name} (copy)`;
    const parentPath = selectedPath.slice(0, -1);
    setTree((t) => insertAt(t, parentPath, copy));
    setNotice(`Duplicated "${node.name}"`);
  }, [selectedPath, tree]);

  const deleteSelected = useCallback(() => {
    if (!selectedPath) return;
    const node = getAt(tree, selectedPath);
    if (!node) return;
    setTree((t) => removeAt(t, selectedPath));
    setSelectedPath(null);
    setNotice(`Deleted "${node.name}"`);
  }, [selectedPath, tree]);

  const expandAll = useCallback(
    (expanded: boolean) => setTree((t) => mapAll(t, (n) => (n.type === "Container" ? { ...n, expanded } : n))),
    [],
  );

  const runImport = useCallback(async (path: string) => {
    try {
      const imported = await importConnectionsFile(path);
      setTree(imported);
      setSelectedPath(null);
      setNotice(`Imported ${imported.length} top-level entries from settings`);
      setModal({
        kind: "info",
        title: "Import complete",
        message: `Imported ${imported.length} top-level entries from:\n${path}\n\nThey are shown in the Connections panel and saved to the local config folder.`,
      });
    } catch (e) {
      setModal({ kind: "info", title: "Import failed", message: String(e) });
    }
  }, []);

  const runExport = useCallback(
    async (path: string) => {
      try {
        await exportConnectionsFile(path, tree);
        setModal({
          kind: "info",
          title: "Export complete",
          message: `Wrote WinSSHTerm-format connections.xml to:\n${path}`,
        });
      } catch (e) {
        setModal({ kind: "info", title: "Export failed", message: String(e) });
      }
    },
    [tree],
  );

  /* -------------------------------- menus ------------------------------- */

  const menus: MenuDef[] = useMemo(
    () => [
      {
        label: "File",
        items: [
          {
            kind: "item",
            label: "Import connections…",
            hint: "connections.xml",
            onClick: () =>
              setModal({
                kind: "path",
                action: "import",
                title: "Import WinSSHTerm connections / settings",
                value: "/home/sam/winsshterm-import/connections.xml",
              }),
          },
          {
            kind: "item",
            label: "Export connections…",
            hint: "connections.xml",
            onClick: () =>
              setModal({
                kind: "path",
                action: "export",
                title: "Export connections in WinSSHTerm format",
                value: `${configDir}/connections-export.xml`,
              }),
          },
          { kind: "divider" },
          {
            kind: "item",
            label: "Master password…",
            hint: "v0.2",
            onClick: () =>
              setModal({
                kind: "info",
                title: "Master password",
                message:
                  "The encrypted vault (Argon2id + AES-256-GCM) lands next, with the " +
                  "settings importer that carries your stored passwords across.",
              }),
          },
          {
            kind: "item",
            label: "Config folder…",
            hint: configDir ? "open" : "",
            onClick: () =>
              setModal({ kind: "info", title: "Config folder", message: configDir }),
          },
          { kind: "divider" },
          { kind: "item", label: "Exit", onClick: () => window.close() },
        ],
      },
      {
        label: "View",
        items: [
          { kind: "item", label: "Expand all", onClick: () => expandAll(true) },
          { kind: "item", label: "Collapse all", onClick: () => expandAll(false) },
          { kind: "divider" },
          { kind: "item", label: "Connections panel", hint: "always on", disabled: true, onClick: () => {} },
          { kind: "item", label: "Configuration panel", hint: "always on", disabled: true, onClick: () => {} },
        ],
      },
      {
        label: "Navigate",
        items: [
          { kind: "item", label: "New session", hint: "Ctrl+N", onClick: () => addSession(false) },
          { kind: "item", label: "New folder", onClick: () => addSession(true) },
          { kind: "divider" },
          {
            kind: "item",
            label: "Connect",
            disabled: !selected || selected.type === "Container",
            onClick: () => selectedPath && openSession(selectedPath),
          },
          {
            kind: "item",
            label: "Copy Files (SFTP)…",
            hint: "next",
            disabled: !selected || selected.type === "Container",
            onClick: () =>
              setModal({
                kind: "info",
                title: "Copy Files / SFTP commander",
                message:
                  "The dual-pane commander (WinSCP-style, F5/F6/F7/F8) is the next " +
                  "milestone. The session model already carries cfProt=sftp per host.",
              }),
          },
        ],
      },
      {
        label: "Tools",
        items: [
          {
            kind: "item",
            label: "SSH key manager…",
            hint: "Pageant",
            onClick: () =>
              setModal({
                kind: "info",
                title: "SSH key manager",
                message:
                  "Pageant-equivalent lands with the ssh-agent UI: list fingerprints, " +
                  "add/remove keys, and convert .ppk → .pem automatically via puttygen.",
              }),
          },
          { kind: "divider" },
          { kind: "item", label: "New local shell", onClick: openLocalShell },
        ],
      },
      {
        label: "Help",
        items: [
          {
            kind: "item",
            label: "About NuxSSHTerm",
            onClick: () =>
              setModal({
                kind: "info",
                title: "About",
                message:
                  "NuxSSHTerm — v0.1 development build.\n\n" +
                  "A native Linux reimplementation of WinSSHTerm (no Wine).\n" +
                  "Sessions via system OpenSSH over a PTY; UI modelled on WinSSHTerm 2.43.x.",
              }),
          },
        ],
      },
      { label: "Cons", items: [{ kind: "item", label: "Console options…", hint: "v0.2", disabled: true, onClick: () => {} }] },
    ],
    [addSession, configDir, expandAll, openLocalShell, openSession, selected, selectedPath],
  );

  /* ---------------------------- keyboard ---------------------------- */

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "t") {
        e.preventDefault();
        openLocalShell();
      } else if (e.ctrlKey && e.key.toLowerCase() === "w" && activeTab) {
        e.preventDefault();
        closeTab(activeTab);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeTab, closeTab, openLocalShell]);

  /* ------------------------------ render ---------------------------- */

  const treeCb: TreeCallbacks = {
    onSelect: (path) => setSelectedPath(path),
    onToggle: (path) => {
      const n = getAt(tree, path);
      if (n?.type === "Container") setTree((t) => updateAt(t, path, { expanded: !n.expanded }));
    },
    onOpen: openSession,
    onContextMenu: (e, path) => {
      e.preventDefault();
      setSelectedPath(path);
      setCtx({ x: e.clientX, y: e.clientY, path });
    },
  };

  const ctxNode = ctx ? getAt(tree, ctx.path) : null;

  return (
    <div
      className="app"
      onMouseDown={() => {
        setCtx(null);
        setTabCtx(null);
      }}
    >
      <MenuBar menus={menus} quick={{ paste: () => void pasteIntoActive(), target: currentTab?.target ?? "" }} />

      <div className="body">
        <div className="leftcol">
          <div className="panel panel-conn">
            <div className="panel-title">
              <span>Connections</span>
              <span className="spacer" />
              <button className="pbtn" title="Collapse all" onClick={() => expandAll(false)}>▼</button>
              <button className="pbtn" title="Expand all" onClick={() => expandAll(true)}>▲</button>
              <button className="pbtn" title="Hide">✕</button>
            </div>
            <div className="panel-toolbar">
              <button className="tbtn" title="Back" disabled>◀</button>
              <button className="tbtn" title="Forward" disabled>▶</button>
              <button className="tbtn" title="Collapse all" onClick={() => expandAll(false)}>▾</button>
              <button className="tbtn" title="Expand all" onClick={() => expandAll(true)}>▴</button>
              <button className="tbtn" title="Refresh" onClick={() => setNotice("Tree refreshed")}>⟳</button>
              <button className="tbtn" title="Search (v0.2)" disabled>🔍</button>
            </div>
            <SessionTree tree={tree} selectedPath={selectedPath} cb={treeCb} />
          </div>

          <ConfigPanel
            node={selected}
            onChange={(patch) => {
              if (!selectedPath) return;
              setTree((t) => updateAt(t, selectedPath, patch));
            }}
          />
        </div>

        <div className="docarea">
          <TabStrip
            tabs={tabs}
            activeId={activeTab}
            onSelect={setActiveTab}
            onClose={closeTab}
            onNewLocal={openLocalShell}
            onContextMenu={(e, id) => {
              e.preventDefault();
              setCtx(null);
              setTabCtx({ x: e.clientX, y: e.clientY, id });
            }}
          />

          <div className="term-area">
            {tabs.length === 0 && (
              <div className="workspace-hint">
                Double-click a connection in the Connections panel to open a terminal tab.
                <br />
                Import your WinSSHTerm settings via <b>File → Import connections…</b>
              </div>
            )}
            {tabs.map((t) => (
              <TerminalView key={t.id} tab={t} active={t.id === activeTab} />
            ))}
          </div>
        </div>
      </div>

      <StatusBar tab={currentTab} configDir={configDir} sessionCount={connCount} notice={notice} />

      {ctx && ctxNode && ctx.path[0] >= 0 && (
        <div className="menu-pop" style={{ left: ctx.x, top: ctx.y }} onMouseDown={(e) => e.stopPropagation()}>
          {ctxNode.type === "Connection" && (
            <>
              <div className="menu-entry" onClick={() => { setCtx(null); openSession(ctx.path); }}>
                <span>Connect</span>
              </div>
              <div
                className="menu-entry"
                onClick={() => {
                  setCtx(null);
                  setModal({
                    kind: "info",
                    title: "Copy Files / SFTP commander",
                    message: "Dual-pane SFTP commander is the next milestone.",
                  });
                }}
              >
                <span>Copy Files</span>
                <span className="hint">SFTP</span>
              </div>
              <div className="menu-divider" />
            </>
          )}
          {ctxNode.type === "Container" && (
            <>
              <div className="menu-entry" onClick={() => { setCtx(null); addSession(false); }}>
                <span>New session here</span>
              </div>
              <div className="menu-entry" onClick={() => { setCtx(null); addSession(true); }}>
                <span>New folder here</span>
              </div>
              <div className="menu-divider" />
            </>
          )}
          <div className="menu-entry" onClick={() => { setCtx(null); duplicateSelected(); }}>
            <span>Duplicate</span>
          </div>
          <div className="menu-entry" onClick={() => { setCtx(null); deleteSelected(); }}>
            <span>Delete</span>
          </div>
          <div className="menu-divider" />
          <div
            className="menu-entry"
            onClick={() => {
              const p = ctx.path.slice(0, -1);
              setCtx(null);
              setTree((t) => insertAt(t, p, emptyNode("New session", "Connection")));
            }}
          >
            <span>Add sibling session</span>
          </div>
        </div>
      )}

      {tabCtx && (
        <div
          className="menu-pop"
          style={{ left: tabCtx.x, top: tabCtx.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <div className="menu-entry" onClick={() => { setTabCtx(null); closeTab(tabCtx.id); }}>
            <span>Close</span>
            <span className="hint">Ctrl+W</span>
          </div>
          <div className="menu-entry" onClick={() => { setTabCtx(null); closeOthers(tabCtx.id); }}>
            <span>Close Others</span>
          </div>
          <div className="menu-entry" onClick={() => { setTabCtx(null); closeAllTabs(); }}>
            <span>Close All</span>
          </div>
          <div className="menu-divider" />
          <div className="menu-entry" onClick={() => { setTabCtx(null); openLocalShell(); }}>
            <span>New local shell</span>
          </div>
        </div>
      )}

      {modal && modal.kind === "path" && (
        <div className="modal-backdrop" onMouseDown={() => setModal(null)}>
          <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
            <h3>{modal.title}</h3>
            <div className="modal-body">
              <input
                type="text"
                autoFocus
                value={modal.value}
                onChange={(e) => setModal({ ...modal, value: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    const v = modal.value;
                    setModal(null);
                    void (modal.action === "import" ? runImport(v) : runExport(v));
                  }
                }}
              />
              <p className="muted" style={{ marginBottom: 0 }}>
                Path on this machine. WinSSHTerm writes <span className="mono">connections.xml</span> /
                <span className="mono"> WinSSHTerm.settings</span>.
              </p>
            </div>
            <div className="modal-actions">
              <button className="btn" onClick={() => setModal(null)}>Cancel</button>
              <button
                className="btn primary"
                onClick={() => {
                  const v = modal.value;
                  setModal(null);
                  void (modal.action === "import" ? runImport(v) : runExport(v));
                }}
              >
                {modal.action === "import" ? "Import" : "Export"}
              </button>
            </div>
          </div>
        </div>
      )}

      {modal && modal.kind === "info" && (
        <div className="modal-backdrop" onMouseDown={() => setModal(null)}>
          <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
            <h3>{modal.title}</h3>
            <div className="modal-body">
              <pre style={{ margin: 0, whiteSpace: "pre-wrap", font: "inherit" }}>{modal.message}</pre>
            </div>
            <div className="modal-actions">
              <button className="btn primary" onClick={() => setModal(null)}>OK</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}