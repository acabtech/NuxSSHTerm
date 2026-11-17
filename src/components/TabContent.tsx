// Renders one tab's live view (xterm terminal or SFTP commander).
//
// v0.5.0 mount model: the App keeps exactly ONE mounted TabContent per open
// tab (keyed by tab.id, in a stable staging container), so the underlying
// Terminal/PTY/SFTP session is created once and survives every chrome
// change. The *visible DOM* is relocated with createPortal into a leaf pane
// slot — moving a React subtree between portals never unmounts it, so live
// sessions survive tiling, tab drags and page switches.

import { createPortal } from "react-dom";
import type { Tab } from "../types";
import type { TerminalSettings } from "../types";
import { TerminalView } from "./TerminalView";
import { CommanderView } from "./CommanderView";

interface Props {
  tab: Tab;
  /** The leaf slot div this tab is currently displayed in (null = parked). */
  target: HTMLElement | null;
  /** True when this tab's leaf is the active one (drives focus + refit). */
  active: boolean;
  term: TerminalSettings;
  onToast: (msg: string, kind?: "info" | "success" | "error") => void;
  /** Ctrl+T in the commander opens a terminal tab for the same host. */
  onOpenTerminal: (tab: Tab) => void;
}

export function TabContent({ tab, target, active, term, onToast, onOpenTerminal }: Props) {
  const view =
    tab.kind === "commander" ? (
      <CommanderView tab={tab} active={active} onToast={onToast} onOpenTerminal={onOpenTerminal} />
    ) : (
      <TerminalView tab={tab} active={active} term={term} />
    );
  if (!target) return view;
  return createPortal(view, target);
}
