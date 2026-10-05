// Menu definition builder. Keeps the (large) WinSSHTerm menu tree out of App.tsx
// and centralises the dispatch of each menu item to an action callback.

import type { MenuDef } from "../components/MenuBar";
import type { SessionNode } from "../types";

export interface MenuActions {
  configDir: string;
  selected: SessionNode | null;
  selectedPath: number[] | null;
  onImport: () => void;
  onExport: () => void;
  onMasterPassword: () => void;
  onLockVault: () => void;
  vaultUnlocked: boolean;
  onConfigFolder: () => void;
  onExpandAll: (expanded: boolean) => void;
  onAddSession: (asContainer: boolean) => void;
  onConnect: () => void;
  onCopyFiles: () => void;
  onKeyManager: () => void;
  onOpenLocalShell: () => void;
  onAbout: () => void;
}

export function buildMenus(a: MenuActions): MenuDef[] {
  const canConnect = !!a.selected && a.selected.type !== "Container";

  return [
    {
      label: "File",
      items: [
        {
          kind: "item",
          label: "Import connections…",
          hint: "connections.xml",
          onClick: a.onImport,
        },
        {
          kind: "item",
          label: "Export connections…",
          hint: "connections.xml",
          onClick: a.onExport,
        },
        { kind: "divider" },
        {
          kind: "item",
          label: "Master password…",
          onClick: a.onMasterPassword,
        },
        {
          kind: "item",
          label: "Lock vault",
          hint: a.vaultUnlocked ? "unlocked" : "",
          disabled: !a.vaultUnlocked,
          onClick: a.onLockVault,
        },
        {
          kind: "item",
          label: "Config folder…",
          hint: a.configDir ? "open" : "",
          onClick: a.onConfigFolder,
        },
        { kind: "divider" },
        { kind: "item", label: "Exit", onClick: () => window.close() },
      ],
    },
    {
      label: "View",
      items: [
        { kind: "item", label: "Expand all", onClick: () => a.onExpandAll(true) },
        { kind: "item", label: "Collapse all", onClick: () => a.onExpandAll(false) },
        { kind: "divider" },
        { kind: "item", label: "Connections panel", hint: "always on", disabled: true, onClick: () => {} },
        { kind: "item", label: "Configuration panel", hint: "always on", disabled: true, onClick: () => {} },
      ],
    },
    {
      label: "Navigate",
      items: [
        { kind: "item", label: "New session", hint: "Ctrl+N", onClick: () => a.onAddSession(false) },
        { kind: "item", label: "New folder", onClick: () => a.onAddSession(true) },
        { kind: "divider" },
        {
          kind: "item",
          label: "Connect",
          disabled: !canConnect,
          onClick: a.onConnect,
        },
        {
          kind: "item",
          label: "Copy Files (SFTP)…",
          hint: "next",
          disabled: !canConnect,
          onClick: a.onCopyFiles,
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
          onClick: a.onKeyManager,
        },
        { kind: "divider" },
        { kind: "item", label: "New local shell", onClick: a.onOpenLocalShell },
      ],
    },
    {
      label: "Help",
      items: [{ kind: "item", label: "About NuxSSHTerm", onClick: a.onAbout }],
    },
    {
      label: "Cons",
      items: [{ kind: "item", label: "Console options…", hint: "v0.2", disabled: true, onClick: () => {} }],
    },
  ];
}