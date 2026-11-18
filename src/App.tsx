// App shell (v0.5.0 modernization): dark design-token chrome, tileable
// panes (WorkArea + useLayout), draggable tabs, live settings (useSettings)
// and the new six-menu surface (menuActions).
//
// Mount model: every open tab is mounted EXACTLY ONCE inside a hidden
// `.content-stage` container (so PTY/SFTP sessions survive tab switches,
// tiling changes and the workspace/settings page switch). The visible DOM
// of each tab is relocated with createPortal into its leaf's `.pane-slot`
// — see TabContent.tsx. `activeLeafId` is the single source of truth for
// "which tab is current"; useTabs' own activeTab is synced to it so
// clipboard/paste helpers keep working.

import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { MenuBar } from "./components/MenuBar";
import { ConfigPanel } from "./components/ConfigPanel";
import { SessionTree, type TreeCallbacks } from "./components/SessionTree";
import { StatusBar } from "./components/StatusBar";
import { TabStrip } from "./components/TabStrip";
import { TabContent } from "./components/TabContent";
import { WorkArea } from "./components/WorkArea";
import { SettingsPage } from "./components/SettingsPage";
import { Icon } from "./components/Icon";
import { ImportWizard } from "./components/ImportWizard";
import { ToastStack } from "./components/Toast";
import { VaultModal, type VaultModalKind } from "./components/VaultModal";
import { KeyManagerModal } from "./components/KeyManagerModal";
import { useSessionTree } from "./hooks/useSessionTree";
import { useTabs } from "./hooks/useTabs";
import { useToasts, type ToastKind } from "./hooks/useToasts";
import { useVault } from "./hooks/useVault";
import { useSettings } from "./hooks/useSettings";
import { useRevealConfigDir } from "./hooks/useRevealConfigDir";
import {
  collectLeaves,
  findLeafByTab,
  moveTabToLeaf,
  neighborLeaf,
  splitLeaf,
  useLayout,
} from "./hooks/useLayout";
import { buildMenus } from "./lib/menuActions";
import { accentSoftFromHex, normalizeHexColor } from "./lib/color";
import { emptyNode, type SessionNode, type Tab } from "./types";
import { applyPasswords, clearPasswords, getAt, insertAt, pathKey } from "./lib/tree";

const SHORTCUTS = [
  ["Ctrl + T", "New local shell"],
  ["Ctrl + W", "Close current tab"],
  ["Ctrl + Shift + C", "Copy terminal selection"],
  ["Ctrl + V", "Paste into terminal"],
  ["Ctrl + ,", "Toggle Settings page"],
  ["Drag tab → pane", "Move the session (swap)"],
  ["Drag tab → gutter", "Split pane and drop there"],
  ["Drag gutter", "Resize panes"],
];

type Modal =
  | { kind: "path"; action: "export"; title: string; value: string }
  | { kind: "import-sessions" }
  | { kind: "key-manager" }
  | { kind: "info"; title: string; message: string }
  | { kind: "shortcuts" }
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
  const [page, setPage] = useState<"workspace" | "settings">("workspace");

  // notify drives both the status-bar line and the toast stack.
  const notify = useCallback(
    (msg: string, kind: ToastKind = "info") => {
      setNotice(msg);
      push(msg, kind);
    },
    [push],
  );

  const {
    ui,
    update: patchUi,
    updateTerminal,
    updateTransfer,
    updateSidebar,
    save: saveSettings,
  } = useSettings(notify);

  // ---- Connections sidebar: settings-driven width + collapse (issues #10/#7) ----
  const sidebar = ui.sidebar;
  const collapseMode = sidebar.collapseMode === "never" ? "never" : "hover";
  const [railHovered, setRailHovered] = useState(false);
  const railElRef = useRef<HTMLDivElement | null>(null);
  const railCollapseTimer = useRef<number | null>(null);
  const [railResizing, setRailResizing] = useState(false);
  const railDragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  // Rail is expanded when hovered/focused (hover mode) or always pinned (never).
  const railExpanded = collapseMode === "never" || railHovered || railResizing;

  const clearRailCollapseTimer = () => {
    if (railCollapseTimer.current !== null) {
      window.clearTimeout(railCollapseTimer.current);
      railCollapseTimer.current = null;
    }
  };

  const onRailEnter = () => {
    clearRailCollapseTimer();
    setRailHovered(true);
  };
  const onRailLeave = () => {
    if (collapseMode === "never" || railResizing) return;
    clearRailCollapseTimer();
    railCollapseTimer.current = window.setTimeout(() => {
      setRailHovered(false);
      railCollapseTimer.current = null;
    }, Math.max(0, sidebar.collapseDelayMs));
  };

  // Keep hover state coherent if the user switches collapse mode live.
  useEffect(() => {
    if (collapseMode === "never") clearRailCollapseTimer();
  }, [collapseMode]);

  /** Drag-to-resize the expanded sidebar edge (persists width in UiSettings). */
  const startRailResize = (e: React.MouseEvent) => {
    e.preventDefault();
    railDragRef.current = { startX: e.clientX, startWidth: sidebar.width };
    setRailResizing(true);
    document.body.classList.add("rail-resizing");
    const onMove = (ev: MouseEvent) => {
      const d = railDragRef.current;
      if (!d) return;
      const w = Math.min(Math.max(Math.round(d.startWidth + (ev.clientX - d.startX)), 200), 560);
      updateSidebar({ width: w });
    };
    const onUp = () => {
      railDragRef.current = null;
      setRailResizing(false);
      document.body.classList.remove("rail-resizing");
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };
  // ---------------------------------------------------------------------------

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

  const openConfigFolder = useRevealConfigDir(configDir, notify);

  const tabsApi = useTabs(notify);
  const {
    tabs,
    setActiveTab,
    openSession,
    openCommander,
    openTerminalFromSpec,
    openLocalShell,
    closeTab,
    closeTabKeepActive,
    reorderTabs,
    closeOthers,
    closeAllTabs,
    pasteIntoActive,
  } = tabsApi;

  /* ------------------------------ layout ------------------------------ */

  const { layout, setLayout, split, close: closeLeafId, resize, reconcile } = useLayout();
  const [activeLeafId, setActiveLeafId] = useState<string>("l1");

  // leaf slot DOM elements, reported by WorkArea via onSlot.
  const slotEls = useRef(new Map<string, HTMLElement>());
  const [slotsVersion, bumpSlots] = useState(0);

  const onSlot = useCallback((leafId: string, el: HTMLElement | null) => {
    if (el) slotEls.current.set(leafId, el);
    else slotEls.current.delete(leafId);
    bumpSlots((n) => n + 1);
  }, []);

  // The leaf/tab model: activeLeafId is the single source of truth.
  const currentLeaf = collectLeaves(layout).find((l) => l.id === activeLeafId) ?? null;
  const currentTabId = currentLeaf?.tabId ?? null;
  const currentTab = tabs.find((t) => t.id === currentTabId) ?? null;

  // Sync useTabs' own activeTab (clipboard/paste helpers rely on it).
  useEffect(() => {
    setActiveTab(currentTabId);
  }, [currentTabId, setActiveTab]);

  // Reconcile the layout tree whenever the tab set changes: prune leaves of
  // closed tabs, place newly opened tabs (preferring the active leaf).
  const tabIdsKey = tabs.map((t) => t.id).join(",");
  useEffect(() => {
    reconcile(tabs.map((t) => t.id), activeLeafId);
  }, [tabIdsKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Keep the active leaf valid after any prune/normalize.
  useEffect(() => {
    const leaves = collectLeaves(layout);
    if (!leaves.some((l) => l.id === activeLeafId)) {
      const hit = currentTabId ? findLeafByTab(layout, currentTabId) : null;
      setActiveLeafId(hit ?? leaves[0]?.id ?? "l1");
    }
  }, [layout, activeLeafId, currentTabId]);

  // tabId -> portal target (the leaf slot currently showing it, or null = parked)
  const targetForTabId = useMemo(() => {
    const out: Record<string, HTMLElement | null> = {};
    for (const l of collectLeaves(layout)) {
      out[l.tabId ?? ""] = l.tabId ? slotEls.current.get(l.id) ?? null : null;
    }
    return out;
    // slotsVersion re-triggers after callback-refs register new leaf slots
  }, [layout, tabs, page, slotsVersion]);

  /* ----------------------------- handlers ----------------------------- */

  const selectTab = useCallback(
    (id: string) => {
      const leafId = findLeafByTab(layout, id);
      if (leafId) setActiveLeafId(leafId);
      setPage("workspace");
    },
    [layout],
  );

  const handleMoveTab = useCallback(
    (leafId: string, tabId: string) => {
      setLayout((t) => moveTabToLeaf(t, leafId, tabId));
      setActiveLeafId(leafId);
    },
    [setLayout],
  );

  const splitActive = useCallback(
    (dir: "row" | "col") => {
      split(activeLeafId, dir);
    },
    [split, activeLeafId],
  );

  /** Drop a tab onto a gutter: split the neighbor leaf and move the tab in. */
  const splitForTab = useCallback(
    (leafId: string, tabId: string, dir: "row" | "col") => {
      setLayout((t) => {
        const splitNode = splitLeaf(t, leafId, null, dir);
        const after = collectLeaves(splitNode);
        const empty = after.find((l) => l.tabId === null);
        return empty ? moveTabToLeaf(splitNode, empty.id, tabId) : splitNode;
      });
      setActiveLeafId(leafId);
    },
    [setLayout],
  );

  const handleClosePane = useCallback(
    (leafId: string) => {
      const leaf = collectLeaves(layout).find((l) => l.id === leafId);
      const tabId = leaf?.tabId ?? null;
      const nb = neighborLeaf(layout, leafId);
      if (tabId) closeTabKeepActive(tabId);
      closeLeafId(leafId);
      const next = nb && collectLeaves(layout).some((l) => l.id === nb) ? nb : "l1";
      setActiveLeafId(next);
    },
    [layout, closeTabKeepActive, closeLeafId],
  );

  /** Ctrl+W / tab ✕: close the tab; if it was in the active leaf, move focus. */
  const handleCloseTab = useCallback(
    (id: string) => {
      const wasActive = id === currentTabId;
      closeTab(id);
      if (wasActive) {
        const nb = neighborLeaf(layout, activeLeafId);
        if (nb) setActiveLeafId(nb);
      }
    },
    [closeTab, currentTabId, layout, activeLeafId],
  );

  const handleOpenTerminalFromCommander = useCallback(
    (ct: Tab) => {
      openTerminalFromSpec(ct.spec, ct.title.replace(/ · SFTP$/, ""), ct.target, ct.password);
      setPage("workspace");
    },
    [openTerminalFromSpec],
  );

  // When a tab is newly opened (reconcile placed it), focus its leaf once.
  const lastTabCount = useRef(tabs.length);
  useEffect(() => {
    if (tabs.length > lastTabCount.current) {
      const nt = tabs[tabs.length - 1];
      const leafId = findLeafByTab(layout, nt?.id ?? "");
      if (leafId) setActiveLeafId(leafId);
    }
    lastTabCount.current = tabs.length;
  }, [tabIdsKey]); // eslint-disable-line react-hooks/exhaustive-deps

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
      } else if (res.ok) {
        setPage("workspace");
      }
    },
    [openSession],
  );

  const openCommanderChecked = useCallback(
    (node: SessionNode) => {
      const res = openCommander(node);
      if (res.ok) setPage("workspace");
    },
    [openCommander],
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

  // Auto-lock the encrypted vault after a period of inactivity while unlocked.
  useEffect(() => {
    const minutes = ui.vaultAutoLockMinutes;
    if (!vaultStatus.unlocked || !minutes || minutes <= 0) return;

    let timer: number | null = null;
    const ms = minutes * 60_000;
    const arm = () => {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void vaultLock()
          .then(() => notify("Vault auto-locked due to inactivity"))
          .catch((e) => notify(`Auto-lock failed: ${String(e)}`, "error"));
      }, ms);
    };

    const events: Array<keyof WindowEventMap> = [
      "mousemove",
      "mousedown",
      "keydown",
      "touchstart",
      "scroll",
    ];
    const onActivity = () => arm();
    for (const ev of events) window.addEventListener(ev, onActivity, { passive: true });
    arm();

    return () => {
      if (timer !== null) window.clearTimeout(timer);
      for (const ev of events) window.removeEventListener(ev, onActivity);
    };
  }, [vaultStatus.unlocked, ui.vaultAutoLockMinutes, vaultLock, notify]);

  // Settings import is the Phase 2 wizard: preview → map → merge/save.
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
        selected,
        activeTabId: currentTabId,
        currentKind: currentTab?.kind ?? null,
        onNewLocalShell: () => {
          openLocalShell();
          setPage("workspace");
        },
        onNewSession: () =>
          addSession(false, {
            forwardAgent: ui.defaultForwardAgent,
            privateKey: ui.defaultPrivateKey,
          }),
        onNewFolder: () => addSession(true),
        onImport: () => setModal({ kind: "import-sessions" }),
        onExport: () =>
          setModal({
            kind: "path",
            action: "export",
            title: "Export connections in WinSSHTerm format",
            value: `${configDir}/connections-export.xml`,
          }),
        onOpenConfigFolder: openConfigFolder,
        onLockVault: () => {
          void vaultLock()
            .then(() => notify("Vault locked"))
            .catch((e) => notify(`Lock failed: ${String(e)}`, "error"));
        },
        vaultUnlocked: vaultStatus.unlocked,
        onCopy: () => notify("Copy: no active terminal selection"),
        onPaste: () => void pasteIntoActive(),
        onToggleConfig: () => patchUi({ showConfig: !ui.showConfig }),
        onSplitActive: splitActive,
        onWorkspace: () => setPage("workspace"),
        onSettings: () => setPage((p) => (p === "settings" ? "workspace" : "settings")),
        onConnect: () => {
          if (selected) openSessionChecked(selected);
        },
        onDuplicate: duplicateSelected,
        onDisconnect: () => {
          if (currentTabId) handleCloseTab(currentTabId);
        },
        onOpenSftp: () => {
          if (selected) openCommanderChecked(selected);
        },
        onKeyManager: () => setModal({ kind: "key-manager" }),
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
        onShortcuts: () => setModal({ kind: "shortcuts" }),
        onDocs: () =>
          setModal({
            kind: "info",
            title: "Documentation",
            message:
              "Project documentation is available in the repository:\n\n" +
              "• docs/ARCHITECTURE.md — system architecture and roadmap\n" +
              "• docs/UI-SPEC.md — UI behavior and historical parity notes\n" +
              "• plans/DESIGN.md + plans/modernization-plan.md — v0.5.0 design/IA\n\n" +
              "Open the project folder or config/docs location in your editor to view them.",
          }),
        onAbout: () =>
          setModal({
            kind: "info",
            title: "About NuxSSHTerm",
            message:
              "NuxSSHTerm — v0.5.0.\n\n" +
              "A native Linux reimplementation of WinSSHTerm (no Wine).\n" +
              "Sessions via system OpenSSH over a PTY; UI modelled on WinSSHTerm 2.43.x.\n" +
              "Modernized chrome: tileable panes, draggable tabs, persisted appearance settings.",
          }),
      }),
    [
      selected,
      currentTabId,
      currentTab?.kind,
      openLocalShell,
      addSession,
      duplicateSelected,
      configDir,
      openConfigFolder,
      vaultStatus.unlocked,
      vaultStatus.initialized,
      vaultLock,
      pasteIntoActive,
      ui.showConfig,
      patchUi,
      splitActive,
      openSessionChecked,
      handleCloseTab,
      openCommanderChecked,
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
        setPage("workspace");
      } else if (e.ctrlKey && e.key.toLowerCase() === "w" && currentTabId) {
        e.preventDefault();
        handleCloseTab(currentTabId);
      } else if (e.ctrlKey && e.key === ",") {
        e.preventDefault();
        setPage((p) => (p === "settings" ? "workspace" : "settings"));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openLocalShell, currentTabId, handleCloseTab]);

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

  const accent = normalizeHexColor(ui.accentColor);
  const appStyle = {
    fontSize: `${Math.round(14 * ui.fontScale)}px`,
    "--accent": accent,
    "--accent-soft": accentSoftFromHex(accent),
  } as CSSProperties;

  return (
    <div
      className={`app density-${ui.density}`}
      style={appStyle}
      onMouseDown={() => {
        setCtx(null);
        setTabCtx(null);
      }}
    >
      <MenuBar menus={menus} page={page} />

      {/* Every live tab view mounts exactly once, here; portals relocate the
          DOM into the active pane slots. */}
      <div className="content-stage" aria-hidden={page === "settings"}>
        {tabs.map((t) => (
          <TabContent
            key={t.id}
            tab={t}
            target={targetForTabId[t.id] ?? null}
            active={t.id === currentTabId && page === "workspace"}
            term={ui.terminal}
            transfer={ui.transfer}
            onToast={notify}
            onOpenTerminal={handleOpenTerminalFromCommander}
          />
        ))}
      </div>

      {page === "settings" ? (
        <SettingsPage
          ui={ui}
          onPatch={patchUi}
          onPatchTerminal={updateTerminal}
          onPatchTransfer={updateTransfer}
          onPatchSidebar={updateSidebar}
          onSave={() => void saveSettings()}
          onBack={() => setPage("workspace")}
        />
      ) : (
        <div className="workspace">
          <div
            ref={railElRef}
            className={`rail${railExpanded ? " expanded" : ""}${railResizing ? " resizing" : ""}`}
            style={{ width: railExpanded ? sidebar.width : undefined, "--rail-expanded-width": `${sidebar.width}px` } as React.CSSProperties}
            onMouseEnter={onRailEnter}
            onMouseLeave={onRailLeave}
          >
            <div className="rail-inner">
              <div className="rail-collapsed-icons">
                <div className="rail-icon active" title="Connections">
                  <Icon name="server" size={17} />
                </div>
                {ui.showConfig && (
                  <button
                    className="rail-icon"
                    title="Open configuration (Ctrl + ,)"
                    onClick={() => setPage("settings")}
                  >
                    <Icon name="settings" size={17} />
                  </button>
                )}
              </div>
              <div className="rail-content">
                <div className="rail-head">
                  <div className="rail-title">
                    <Icon name="server" size={14} /> Connections
                  </div>
                  <div className="pane-tools">
                    <button className="icon-btn" title="Collapse all" onClick={() => expandAll(false)}>
                      <Icon name="chev" size={13} />
                    </button>
                    <button className="icon-btn" title="Expand all" onClick={() => expandAll(true)}>
                      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden style={{ transform: "rotate(180deg)" }}>
                        <path d="M6 9l6 6 6-6" />
                      </svg>
                    </button>
                  </div>
                </div>
                <SessionTree tree={tree} selectedPath={selectedPath} cb={treeCb} />
                <div className="rail-actions">
                  <button
                    className="btn primary"
                    disabled={!selected || selected.type === "Container"}
                    onClick={() => selected && openSessionChecked(selected)}
                  >
                    <Icon name="terminal" size={13} /> Connect
                  </button>
                  <button
                    className="btn"
                    disabled={!selected || selected.type === "Container"}
                    onClick={() => selected && openCommanderChecked(selected)}
                  >
                    <Icon name="transfer" size={13} /> SFTP
                  </button>
                </div>
                {ui.showConfig && (
                  <div className="config-block">
                    <div className="config-head">
                      <Icon name="settings" size={13} /> Configuration
                    </div>
                    <ConfigPanel
                      node={selected}
                      onChange={handleUpdateNode}
                      vaultUnlocked={vaultStatus.unlocked}
                    />
                  </div>
                )}
              </div>
            </div>
          </div>

          {railExpanded && (
            <div
              className="rail-resize-handle"
              title="Drag to resize sidebar"
              onMouseDown={startRailResize}
            />
          )}

          <main className="work" aria-label="Terminal work area">
            <TabStrip
              tabs={tabs}
              activeId={currentTabId}
              onSelect={selectTab}
              onClose={handleCloseTab}
              onNewLocal={() => {
                openLocalShell();
              }}
              onReorder={reorderTabs}
              onContextMenu={(e, id) => {
                e.preventDefault();
                setCtx(null);
                setTabCtx({ x: e.clientX, y: e.clientY, id });
              }}
            />
            <WorkArea
              node={layout}
              tabs={tabs}
              activeLeafId={activeLeafId}
              onSplit={(leafId, dir) => {
                split(leafId, dir);
                setActiveLeafId(leafId);
              }}
              onClosePane={handleClosePane}
              onMoveTab={handleMoveTab}
              onSplitForTab={splitForTab}
              onResize={resize}
              onSlot={onSlot}
              onNewShell={openLocalShell}
            />
          </main>
        </div>
      )}

      <StatusBar
        tab={currentTab}
        configDir={configDir}
        sessionCount={connCount}
        notice={notice}
        vault={vaultStatus.initialized ? (vaultStatus.unlocked ? "unlocked" : "locked") : "unset"}
      />

      {ctx && ctxNode && ctx.path[0] >= 0 && (
        <div className="menu-pop ctx" style={{ left: ctx.x, top: ctx.y }} onMouseDown={(e) => e.stopPropagation()}>
          {ctxNode.type === "Connection" && (
            <>
              <div className="menu-item" onClick={() => { setCtx(null); openSessionChecked(ctxNode); }}>
                <span>Connect</span>
              </div>
              <div className="menu-item" onClick={() => { setCtx(null); openCommanderChecked(ctxNode); }}>
                <span>Copy Files</span>
                <span className="hint">SFTP</span>
              </div>
              <div className="menu-sep" />
            </>
          )}
          {ctxNode.type === "Container" && (
            <>
              <div
                className="menu-item"
                onClick={() => {
                  setCtx(null);
                  addSession(false, {
                    forwardAgent: ui.defaultForwardAgent,
                    privateKey: ui.defaultPrivateKey,
                  });
                }}
              >
                <span>New session here</span>
              </div>
              <div className="menu-item" onClick={() => { setCtx(null); addSession(true); }}>
                <span>New folder here</span>
              </div>
              <div className="menu-sep" />
            </>
          )}
          <div className="menu-item" onClick={() => { setCtx(null); duplicateSelected(); }}>
            <span>Duplicate</span>
          </div>
          <div className="menu-item" onClick={() => { setCtx(null); deleteSelected(); }}>
            <span>Delete</span>
          </div>
          <div className="menu-sep" />
          <div
            className="menu-item"
            onClick={() => {
              const p = ctx.path.slice(0, -1);
              setCtx(null);
              setTree((t) => {
                const node = emptyNode("New session", "Connection");
                node.forward_agent = ui.defaultForwardAgent;
                node.private_key = ui.defaultPrivateKey;
                return insertAt(t, p, node);
              });
            }}
          >
            <span>Add sibling session</span>
          </div>
        </div>
      )}

      {tabCtx && (
        <div
          className="menu-pop ctx"
          style={{ left: tabCtx.x, top: tabCtx.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <div className="menu-item" onClick={() => { setTabCtx(null); handleCloseTab(tabCtx.id); }}>
            <span>Close</span>
            <span className="hint">Ctrl+W</span>
          </div>
          <div className="menu-item" onClick={() => { setTabCtx(null); closeOthers(tabCtx.id); }}>
            <span>Close Others</span>
          </div>
          <div className="menu-item" onClick={() => { setTabCtx(null); closeAllTabs(); }}>
            <span>Close All</span>
          </div>
          <div className="menu-sep" />
          <div className="menu-item" onClick={() => { setTabCtx(null); openLocalShell(); }}>
            <span>New local shell</span>
          </div>
        </div>
      )}

      {modal && modal.kind === "path" && (
        <div className="modal-backdrop" onMouseDown={() => setModal(null)}>
          <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>{modal.title}</h3>
              <button className="icon-btn" aria-label="Close" onClick={() => setModal(null)}>
                <Icon name="close" size={14} />
              </button>
            </div>
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
          initialPath=""
          vaultUnlocked={vaultStatus.unlocked}
          onClose={() => setModal(null)}
          onImport={handleImportSessions}
        />
      )}

      {modal && modal.kind === "key-manager" && (
        <KeyManagerModal onClose={() => setModal(null)} notify={notify} />
      )}

      {modal && modal.kind === "info" && (
        <div className="modal-backdrop" onMouseDown={() => setModal(null)}>
          <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>{modal.title}</h3>
              <button className="icon-btn" aria-label="Close" onClick={() => setModal(null)}>
                <Icon name="close" size={14} />
              </button>
            </div>
            <div className="modal-body">
              <pre style={{ margin: 0, whiteSpace: "pre-wrap", font: "inherit" }}>{modal.message}</pre>
            </div>
            <div className="modal-actions">
              <button className="btn primary" onClick={() => setModal(null)}>OK</button>
            </div>
          </div>
        </div>
      )}

      {modal && modal.kind === "shortcuts" && (
        <div className="modal-backdrop" onMouseDown={() => setModal(null)}>
          <div className="modal" onMouseDown={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>Keyboard Shortcuts</h3>
              <button className="icon-btn" aria-label="Close" onClick={() => setModal(null)}>
                <Icon name="close" size={14} />
              </button>
            </div>
            <div className="modal-body">
              <div className="kvs">
                {SHORTCUTS.map(([k, v]) => (
                  <div key={k}>
                    <span className="mono">{k}</span>
                    <span className="muted">{v}</span>
                  </div>
                ))}
              </div>
            </div>
            <div className="modal-actions">
              <button className="btn primary" onClick={() => setModal(null)}>Close</button>
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
