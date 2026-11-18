// Menu definition builder (v0.5.0): File / Edit / View / Sessions / Tools /
// Help, per plans/modernization-plan.md + the App.jsx mockup. Each item maps
// a mockup action id onto the existing real backend callbacks — no invented
// behavior.

import type { MenuDef } from "../components/MenuBar";
import { termCopySelection } from "../components/TerminalView";
import type { SessionNode } from "../types";
import { quitApp } from "./quit";

export interface MenuActions {
  selected: SessionNode | null;
  /** id of the tab in the active leaf (null = no active tab) */
  activeTabId: string | null;
  currentKind: "terminal" | "commander" | null;
  onNewLocalShell: () => void;
  onNewSession: () => void;
  onNewFolder: () => void;
  onImport: () => void;
  onExport: () => void;
  onOpenConfigFolder: () => void;
  onLockVault: () => void;
  vaultUnlocked: boolean;
  onCopy: () => void;
  onPaste: () => void;
  onToggleConfig: () => void;
  onSplitActive: (dir: "row" | "col") => void;
  onWorkspace: () => void;
  onSettings: () => void;
  onConnect: () => void;
  onDuplicate: () => void;
  onDisconnect: () => void;
  onOpenSftp: () => void;
  onKeyManager: () => void;
  onMasterPassword: () => void;
  onShortcuts: () => void;
  onDocs: () => void;
  onAbout: () => void;
}

export function buildMenus(a: MenuActions): MenuDef[] {
  const canConnect = !!a.selected && a.selected.type !== "Container";
  const hasTab = a.activeTabId !== null;
  const isTerminal = a.currentKind === "terminal";

  return [
    {
      label: "File",
      items: [
        { kind: "item", label: "New Local Shell", hint: "Ctrl+T", onClick: a.onNewLocalShell },
        { kind: "item", label: "New Connection…", onClick: a.onNewSession },
        { kind: "item", label: "New Folder…", onClick: a.onNewFolder },
        { kind: "divider" },
        { kind: "item", label: "Import connections…", hint: "WinSSHTerm", onClick: a.onImport },
        { kind: "item", label: "Export connections…", hint: "connections.xml", onClick: a.onExport },
        { kind: "item", label: "Open config folder", onClick: a.onOpenConfigFolder },
        {
          kind: "item",
          label: "Lock vault",
          hint: a.vaultUnlocked ? "unlocked" : "",
          disabled: !a.vaultUnlocked,
          onClick: a.onLockVault,
        },
        { kind: "divider" },
        { kind: "item", label: "Settings…", hint: "Ctrl+,", onClick: a.onSettings },
        { kind: "divider" },
        { kind: "item", label: "Quit", onClick: quitApp },
      ],
    },
    {
      label: "Edit",
      items: [
        {
          kind: "item",
          label: "Copy",
          hint: "Shift+Ctrl+C",
          disabled: !hasTab || !isTerminal,
          onClick: () => {
            // Prefer the live xterm selection when present.
            if (!a.activeTabId || !termCopySelection(a.activeTabId)) a.onCopy();
          },
        },
        {
          kind: "item",
          label: "Paste",
          hint: "Ctrl+V",
          disabled: !hasTab || !isTerminal,
          onClick: a.onPaste,
        },
        { kind: "item", label: "Find in Terminal", hint: "v0.6", disabled: true, onClick: () => {} },
      ],
    },
    {
      label: "View",
      items: [
        { kind: "item", label: "Toggle Configuration", onClick: a.onToggleConfig },
        { kind: "item", label: "Split Right", hint: "active pane", onClick: () => a.onSplitActive("row") },
        { kind: "item", label: "Split Down", hint: "active pane", onClick: () => a.onSplitActive("col") },
        { kind: "divider" },
        { kind: "item", label: "Workspace", onClick: a.onWorkspace },
        { kind: "item", label: "Settings Page", onClick: a.onSettings },
      ],
    },
    {
      label: "Sessions",
      items: [
        { kind: "item", label: "Connect Selected", disabled: !canConnect, onClick: a.onConnect },
        { kind: "item", label: "Duplicate", disabled: !a.selected, onClick: a.onDuplicate },
        {
          kind: "item",
          label: "Disconnect",
          disabled: !hasTab,
          onClick: a.onDisconnect,
        },
        { kind: "divider" },
        { kind: "item", label: "Open SFTP Commander", disabled: !canConnect, onClick: a.onOpenSftp },
      ],
    },
    {
      label: "Tools",
      items: [
        { kind: "item", label: "SSH Key Manager", hint: "agent", onClick: a.onKeyManager },
        { kind: "item", label: "Master Password…", onClick: a.onMasterPassword },
        { kind: "item", label: "Command palette", hint: "v0.6", disabled: true, onClick: () => {} },
      ],
    },
    {
      label: "Help",
      items: [
        { kind: "item", label: "Keyboard Shortcuts", onClick: a.onShortcuts },
        {
          kind: "item",
          label: "Documentation",
          hint: "docs/",
          onClick: a.onDocs,
        },
        { kind: "item", label: "About NuxSSHTerm", onClick: a.onAbout },
      ],
    },
  ];
}
