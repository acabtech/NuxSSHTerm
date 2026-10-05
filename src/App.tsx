import { useCallback, useEffect, useMemo, useState } from "react";
import { MenuBar } from "./components/MenuBar";
import { ConfigPanel } from "./components/ConfigPanel";
import { SessionTree, type TreeCallbacks } from "./components/SessionTree";
import { StatusBar } from "./components/StatusBar";
import { TabStrip } from "./components/TabStrip";
import { TerminalView } from "./components/TerminalView";
import { CommanderView } from "./components/CommanderView";
import { ImportWizard } from "./components/ImportWizard";
import { ToastStack } from "./components/Toast";
import { VaultModal, type VaultModalKind } from "./components/VaultModal";
import { useSessionTree } from "./hooks/useSessionTree";
import { useTabs } from "./hooks/useTabs";
import { useToasts, type ToastKind } from "./hooks/useToasts";
import { useVault } from "./hooks/useVault";
import { buildMenus } from "./lib/menuActions";
import { emptyNode, type SessionNode } from "./types";
import { applyPasswords, clearPasswords, getAt, insertAt, pathKey } from "./lib/tree";

type Modal =
  | { kind: "path"; action: "export"; title: string; value: string }
  | { kind: "import-sessions" }
  | { kind: "info"; title: string; message: string }
  | { kind: "vault-init" }
  | { kind: "vault-unlock" }
  | { kind: "vault-reset" }
  | null;

export default function App() {
  const { toasts, push, dismiss } = useToasts();
  const [notice, setNotice] = useState("Loaded session tree");
  const [modal, setModal] = useState<Modal>(null);
  const [ctx, setCtx] = useState<{ x: number; y: number; path: number[] } | null>(null);
  const [tabCtx, setTabCtx] = useState<{ x: number; y: number; id: string } | null>(null);

  // notify drives both the status-bar line and the toast stack.
  const notify = useCallback(
    (msg: string, kind: ToastKind = "info") => {
      setNotice(msg);
      push(msg, kind);
    },
    [push],
  );

  const vault = useVault(notify);
  const {
    status: vaultStatus,
    checking: vaultChecking,
    init: vaultInit,
    unlock: vaultUnlock,
    lock: vaultLock,
    reset: vaultReset,
    getPasswords: vaultGetPasswords,
    putPassword: vaultPutPassword,
  } = vault;

  const treeApi = useSessionTree(notify);
  const {
    tree,
    setTree,
    selectedPath,
    setSelectedPath,
    selected,
    connCount,
    configDir,
    updateNode,
    toggleNode,
    addSession,
    duplicateSelected,
    deleteSelected,
    expandAll,
    runExport,
  } = treeApi;

  const tabsApi = useTabs(notify);
  const {
    tabs,
    activeTab,
    setActiveTab,
    currentTab,
    openSession,
    openCommander,
    openTerminalFromSpec,
    openLocalShell,
    closeTab,
    closeOthers,
    closeAllTabs,
    pasteIntoActive,
  } = tabsApi;

  // open a session, showing the "no host configured" modal when needed.
  const openSessionChecked = useCallback(
    (node: SessionNode) => {
      const res = openSession(node);
      if (!res.ok && res.reason === "no-host") {
        setModal({
          kind: "info",
          title: "No host configured",
          message:
            `"${res.name}" has no Host/IP yet.\n\n` +
            `This entry came from the rebuilt tree placeholder. Set Host/IP in the ` +
            `Configuration panel, or import your real WinSSHTerm settings via ` +
            `File → Import connections…`,
        });
      }
    },
    [openSession],
  );

  /* ------------------------------ vault ------------------------------ */

  // On start, prompt to set up or unlock the vault (dismissable).
  useEffect(() => {
    if (vaultChecking) return;
    if (vaultStatus.unlocked) return;
    setModal(vaultStatus.initialized ? { kind: "vault-unlock" } : { kind: "vault-init" });
  }, [vaultChecking, vaultStatus.initialized, vaultStatus.unlocked]);

  // When the vault is unlocked, hydrate connection passwords from it.
  useEffect(() => {
    if (!vaultStatus.unlocked) return;
    (async () => {
      try {
        const map = await vaultGetPasswords();
        setTree((t) => applyPasswords(t, map));
        notify("Vault unlocked — passwords loaded");
      } catch (e) {
        notify(`Failed to load vault passwords: ${String(e)}`, "error");
      }
    })();
  }, [vaultStatus.unlocked, vaultGetPasswords, setTree, notify]);

  // When the vault is locked, drop every password from the in-memory tree.
  useEffect(() => {
    if (vaultStatus.unlocked) return;
    setTree((t) => clearPasswords(t));
  }, [vaultStatus.unlocked, setTree]);

  // Persist password edits to the vault (only meaningful while unlocked).
  const handleUpdateNode = useCallback(
    (patch: Partial<SessionNode>) => {
      if (patch.password !== undefined && vaultStatus.unlocked && selectedPath) {
        const key = pathKey(tree, selectedPath);
        void vaultPutPassword(key, patch.password).catch((e) =>
          notify(`Vault save failed: ${String(e)}`, "error"),
        );
      }
      updateNode(patch);
    },
    [vaultStatus.unlocked, selectedPath, tree, vaultPutPassword, updateNode, notify],
  );

  // Settings import is now the Phase 2 wizard: preview → map → merge/save.
  const handleImportSessions = useCallback(
    (nodes: SessionNode[], summary: { count: number; format: string; warnings: string[]; merge: boolean }) => {
      setTree((t) => (summary.merge ? [...t, ...nodes] : nodes));
      setSelectedPath(null);
      setModal(null);
      notify(
        `Imported ${summary.count} session(s) from ${summary.format}` +
          (summary.merge ? " — appended to the tree" : " — replaced the tree"),
      );
      for (const w of summary.warnings.slice(0, 4)) {
        notify(w, "error");
      }
    },
    [setTree, notify],
  );

  const handleExport = useCallback(
    async (path: string) => {
      const res = await runExport(path);
      if (res.ok) {
        setModal({
          kind: "info",
          title: "Export complete",
          message: `Wrote WinSSHTerm-format connections.xml to:\n${res.path}`,
        });
      } else {
        setModal({ kind: "info", title: "Export failed", message: res.error ?? "Unknown error" });
      }
    },
    [runExport],
  );

  const handleVaultSubmit = useCallback(
    (kind: VaultModalKind, master: string) => {
      if (kind === "vault-init") {
        void vaultInit(master)
          .then(() => {
            setModal(null);
            notify("Vault created and unlocked");
          })
          .catch((e) => notify(`Vault setup failed: ${String(e)}`, "error"));
      } else if (kind === "vault-unlock") {
        void vaultUnlock(master)
          .then(() => {
            setModal(null);
            notify("Vault unlocked");
          })
          .catch((e) => notify(`Unlock failed: ${String(e)}`, "error"));
      } else {
        void vaultReset()
          .then(() => {
            setModal(null);
            notify("Vault reset — stored passwords cleared");
          })
          .catch((e) => notify(`Reset failed: ${String(e)}`, "error"));
      }
    },
    [vaultInit, vaultUnlock, vaultReset, notify],
  );

  /* -------------------------------- menus ------------------------------- */

  const menus = useMemo(
    () =>
      buildMenus({
        configDir,
        selected,
        selectedPath,
        onImport: () => setModal({ kind: "import-sessions" }),
        onExport: () =>
          setModal({
            kind: "path",
            action: "export",
            title: "Export connections in WinSSHTerm format",
            value: `${configDir}/connections-export.xml`,
          }),
        onMasterPassword: () => {
          if (vaultStatus.unlocked) {
            setModal({
              kind: "info",
              title: "Vault",
              message:
                "The vault is unlocked. Use File → Lock vault to drop the in-memory key and " +
                "clear session passwords from the UI.",
            });
          } else {
            setModal(vaultStatus.initialized ? { kind: "vault-unlock" } : { kind: "vault-init" });
          }
        },
        onLockVault: () => {
          void vaultLock()
            .then(() => notify("Vault locked"))
            .catch((e) => notify(`Lock failed: ${String(e)}`, "error"));
        },
        vaultUnlocked: vaultStatus.unlocked,
        onConfigFolder: () => setModal({ kind: "info", title: "Config folder", message: configDir }),
        onExpandAll: expandAll,
        onAddSession: addSession,
        onConnect: () => {
          if (selected) openSessionChecked(selected);
        },
        onCopyFiles: () => {
          if (selected) openCommander(selected);
        },
        onKeyManager: () =>
          setModal({
            kind: "info",
            title: "SSH key manager",
            message:
              "Pageant-equivalent lands with the ssh-agent UI: list fingerprints, " +
              "add/remove keys, and convert .ppk → .pem automatically via puttygen.",
          }),
        onOpenLocalShell: openLocalShell,
        onAbout: () =>
          setModal({
            kind: "info",
            title: "About",
            message:
              "NuxSSHTerm — v0.1 development build.\n\n" +
              "A native Linux reimplementation of WinSSHTerm (no Wine).\n" +
              "Sessions via system OpenSSH over a PTY; UI modelled on WinSSHTerm 2.43.x.",
          }),
      }),
    [
      configDir,
      selected,
      selectedPath,
      expandAll,
      addSession,
      openSessionChecked,
      openLocalShell,
      vaultStatus.unlocked,
      vaultLock,
      notify,
    ],
  );

  /* ---------------------------- keyboard ---------------------------- */

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return; // commander keys (Ctrl+T etc.) handled locally
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
    onToggle: toggleNode,
    onOpen: (path) => {
      const n = getAt(tree, path);
      if (n) openSessionChecked(n);
    },
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
              <button className="tbtn" title="Refresh" onClick={() => notify("Tree refreshed")}>⟳</button>
              <button className="tbtn" title="Search (v0.2)" disabled>🔍</button>
            </div>
            <SessionTree tree={tree} selectedPath={selectedPath} cb={treeCb} />
          </div>

          <ConfigPanel
            node={selected}
            onChange={handleUpdateNode}
            vaultUnlocked={vaultStatus.unlocked}
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
            {tabs.map((t) =>
              t.kind === "commander" ? (
                <CommanderView
                  key={t.id}
                  tab={t}
                  active={t.id === activeTab}
                  onToast={notify}
                  onOpenTerminal={(ct) =>
                    openTerminalFromSpec(
                      ct.spec,
                      ct.title.replace(/ · SFTP$/, ""),
                      ct.target,
                    )
                  }
                />
              ) : (
                <TerminalView key={t.id} tab={t} active={t.id === activeTab} />
              ),
            )}
          </div>
        </div>
      </div>

      <StatusBar
        tab={currentTab}
        configDir={configDir}
        sessionCount={connCount}
        notice={notice}
        vault={vaultStatus.initialized ? (vaultStatus.unlocked ? "unlocked" : "locked") : "unset"}
      />

      {ctx && ctxNode && ctx.path[0] >= 0 && (
        <div className="menu-pop" style={{ left: ctx.x, top: ctx.y }} onMouseDown={(e) => e.stopPropagation()}>
          {ctxNode.type === "Connection" && (
            <>
              <div className="menu-entry" onClick={() => { setCtx(null); openSessionChecked(ctxNode); }}>
                <span>Connect</span>
              </div>
              <div
                className="menu-entry"
                onClick={() => {
                  setCtx(null);
                  openCommander(ctxNode);
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
                    void handleExport(v);
                  }
                }}
              />
              <p className="muted" style={{ marginBottom: 0 }}>
                Path on this machine. The export is written in the WinSSHTerm
                <span className="mono">connections.xml</span> format.
              </p>
            </div>
            <div className="modal-actions">
              <button className="btn" onClick={() => setModal(null)}>Cancel</button>
              <button
                className="btn primary"
                onClick={() => {
                  const v = modal.value;
                  setModal(null);
                  void handleExport(v);
                }}
              >
                Export
              </button>
            </div>
          </div>
        </div>
      )}

      {modal && modal.kind === "import-sessions" && (
        <ImportWizard
          initialPath="/home/sam/winsshterm-import/connections.xml"
          vaultUnlocked={vaultStatus.unlocked}
          onClose={() => setModal(null)}
          onImport={handleImportSessions}
        />
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

      {modal &&
        (modal.kind === "vault-init" ||
          modal.kind === "vault-unlock" ||
          modal.kind === "vault-reset") && (
          <VaultModal
            kind={modal.kind}
            onClose={() => setModal(null)}
            onSubmit={(master) => handleVaultSubmit(modal.kind, master)}
            onForgot={
              modal.kind === "vault-unlock" ? () => setModal({ kind: "vault-reset" }) : undefined
            }
          />
        )}

      <ToastStack toasts={toasts} onDismiss={dismiss} />
    </div>
  );
}