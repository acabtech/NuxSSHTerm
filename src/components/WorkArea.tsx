// Tileable work area (v0.5.0): renders the layout tree as panes with a
// "slot" div per leaf. TabContent portals the live terminal/commander DOM
// into whichever slot its tab currently occupies — see TabContent.tsx.
//
// Drag and drop:
//   * a tab dragged onto a pane   → move that tab into the pane (swap)
//   * a tab dragged onto a gutter → split the neighbor pane, drop tab inside
//   * gutters are mouse-draggable to resize the split they sit in.

import { useCallback, useRef, useState } from "react";
import type { LayoutNode } from "../types";
import type { Tab } from "../types";
import { collectLeaves } from "../hooks/useLayout";
import { Icon } from "./Icon";

export const DND_TAB = "text/nux-tab";

interface WorkAreaProps {
  node: LayoutNode;
  tabs: Tab[];
  activeLeafId: string | null;
  onSplit: (leafId: string, dir: "row" | "col") => void;
  onClosePane: (leafId: string) => void;
  onMoveTab: (leafId: string, tabId: string) => void;
  /** Drop a tab onto a gutter: split the leaf on the given side. */
  onSplitForTab: (leafId: string, tabId: string, dir: "row" | "col") => void;
  onResize: (splitId: string, sizes: number[]) => void;
  onSlot: (leafId: string, el: HTMLElement | null) => void;
  onNewShell: () => void;
}

function leafTab(tabs: Tab[], node: Extract<LayoutNode, { kind: "leaf" }>): Tab | undefined {
  return node.tabId ? tabs.find((t) => t.id === node.tabId) : undefined;
}

export function WorkArea(props: WorkAreaProps) {
  const leafCount = collectLeaves(props.node).length;
  const render = (node: LayoutNode): React.ReactNode => {
    if (node.kind === "leaf") {
      return <LeafPane {...props} node={node} canClose={leafCount > 1 || node.tabId !== null} />;
    }
    return <SplitPane {...props} node={node} render={render} />;
  };
  return <div className="tile-area">{render(props.node)}</div>;
}

/* ------------------------------- leaf pane ------------------------------- */

function LeafPane({
  node,
  tabs,
  activeLeafId,
  canClose,
  onSplit,
  onClosePane,
  onMoveTab,
  onSlot,
  onNewShell,
}: WorkAreaProps & { node: Extract<LayoutNode, { kind: "leaf" }>; canClose: boolean }) {
  const [over, setOver] = useState(false);
  const tab = leafTab(tabs, node);
  const active = activeLeafId === node.id;

  const slotCb = useCallback(
    (el: HTMLElement | null) => {
      onSlot(node.id, el);
    },
    [node.id, onSlot],
  );

  return (
    <div
      className={`pane${active ? " active" : ""}${over ? " drag-over" : ""}`}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes(DND_TAB)) {
          e.preventDefault();
          setOver(true);
        }
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        const dragged = e.dataTransfer.getData(DND_TAB);
        if (dragged) {
          e.preventDefault();
          onMoveTab(node.id, dragged);
        }
      }}
    >
      <div className="pane-head">
        <span className="pane-title">
          {tab ? (
            <>
              <Icon name={tab.kind === "commander" ? "transfer" : "terminal"} size={13} />{" "}
              {tab.kind === "commander" ? "SFTP Commander" : "Terminal"} — {tab.title}
            </>
          ) : (
            "Empty pane"
          )}
        </span>
        <div className="pane-tools">
          <button className="icon-btn" title="Split right" aria-label="Split right" onClick={() => onSplit(node.id, "row")}>
            <Icon name="splitH" size={13} />
          </button>
          <button className="icon-btn" title="Split down" aria-label="Split down" onClick={() => onSplit(node.id, "col")}>
            <Icon name="splitV" size={13} />
          </button>
          {canClose && (
            <button className="icon-btn" title="Close pane" aria-label="Close pane" onClick={() => onClosePane(node.id)}>
              <Icon name="close" size={13} />
            </button>
          )}
        </div>
      </div>
      <div className="pane-body">
        <div className="pane-slot" ref={slotCb} />
        {!tab && (
          <div className="pane-empty">
            No session in this pane. Drag a tab here,{" "}
            <button className="linklike" onClick={onNewShell}>
              open a local shell
            </button>{" "}
            or double-click a connection.
          </div>
        )}
      </div>
    </div>
  );
}

/* ------------------------------- split node ------------------------------ */

function SplitPane({
  node,
  onResize,
  onSplitForTab,
  render,
}: WorkAreaProps & { node: Extract<LayoutNode, { kind: "split" }>; render: (n: LayoutNode) => React.ReactNode }) {
  const isRow = node.dir === "row";
  const dragRef = useRef<{ gutter: number; rect: DOMRect; sizes: number[]; startX: number; startY: number } | null>(null);

  const startResize = (e: React.MouseEvent, gutter: number, splitEl: HTMLElement) => {
    e.preventDefault();
    dragRef.current = {
      gutter,
      rect: splitEl.getBoundingClientRect(),
      sizes: [...node.sizes],
      startX: e.clientX,
      startY: e.clientY,
    };
    const move = (ev: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const total = isRow ? d.rect.width : d.rect.height;
      const deltaPx = isRow ? ev.clientX - d.startX : ev.clientY - d.startY;
      const sum = d.sizes[d.gutter] + d.sizes[d.gutter + 1];
      const moved = d.sizes[d.gutter] + (deltaPx / total) * 100;
      const a = Math.min(Math.max(moved, 15), sum - 15);
      const next = [...d.sizes];
      next[d.gutter] = a;
      next[d.gutter + 1] = sum - a;
      onResize(node.id, next);
    };
    const up = () => {
      dragRef.current = null;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className={`split ${isRow ? "row" : "col"}`} data-split={node.id}>
      {node.children.map((child, idx) => {
        const prev = idx > 0 ? node.children[idx - 1] : null;
        return (
          <div key={child.id} className="split-child" style={{ flexGrow: node.sizes[idx] ?? 1, flexBasis: 0 }}>
            {prev && (
              <div
                className={`gutter ${isRow ? "v" : "h"}`}
                title="Drag to resize · drop a tab here to split"
                onMouseDown={(e) => {
                  const el =
                    (e.currentTarget.closest(".split") as HTMLElement) ?? e.currentTarget.parentElement!;
                  startResize(e, idx - 1, el);
                }}
                onDragOver={(e) => {
                  if (e.dataTransfer.types.includes(DND_TAB)) e.preventDefault();
                }}
                onDrop={(e) => {
                  const dragged = e.dataTransfer.getData(DND_TAB);
                  if (!dragged) return;
                  e.preventDefault();
                  e.stopPropagation();
                  // Split the leaf on the left/upper side of this gutter.
                  const targetLeaf = prev.kind === "leaf" ? prev : collectLeaves(prev).slice(-1)[0];
                  if (targetLeaf) onSplitForTab(targetLeaf.id, dragged, isRow ? "row" : "col");
                }}
              />
            )}
            {render(child)}
          </div>
        );
      })}
    </div>
  );
}
