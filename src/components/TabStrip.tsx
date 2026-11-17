import { useState } from "react";
import type { Tab } from "../types";
import { Icon } from "./Icon";
import { DND_TAB } from "./WorkArea";

export function TabStrip({
  tabs,
  activeId,
  onSelect,
  onClose,
  onNewLocal,
  onContextMenu,
  onReorder,
}: {
  tabs: Tab[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onNewLocal: () => void;
  onContextMenu: (e: React.MouseEvent, id: string) => void;
  /** Move a dragged tab so it lands in `toId`'s position. */
  onReorder: (fromId: string, toId: string) => void;
}) {
  const [dragOverId, setDragOverId] = useState<string | null>(null);

  return (
    <div className="tabs" role="tablist" aria-label="Open tabs">
      {tabs.map((t) => (
        <div
          key={t.id}
          role="tab"
          aria-selected={t.id === activeId}
          tabIndex={0}
          className={`tab${t.id === activeId ? " active" : ""}${dragOverId === t.id ? " drag-over" : ""}`}
          draggable
          onDragStart={(e) => {
            e.dataTransfer.setData(DND_TAB, t.id);
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragOver={(e) => {
            if (e.dataTransfer.types.includes(DND_TAB)) {
              e.preventDefault();
              setDragOverId(t.id);
            }
          }}
          onDragLeave={() => setDragOverId((cur) => (cur === t.id ? null : cur))}
          onDrop={(e) => {
            e.preventDefault();
            setDragOverId(null);
            const from = e.dataTransfer.getData(DND_TAB);
            if (from && from !== t.id) onReorder(from, t.id);
          }}
          onMouseDown={() => onSelect(t.id)}
          onKeyDown={(e) => e.key === "Enter" && onSelect(t.id)}
          onContextMenu={(e) => onContextMenu(e, t.id)}
          title={`${t.title} — ${t.target}`}
        >
          <Icon name={t.kind === "commander" ? "transfer" : "terminal"} size={13} />
          <span className="tab-label">{t.title}</span>
          <span className={`tab-dot ${t.exited ? "dead" : "pending"}`} />
          <button
            className="tab-close"
            aria-label={`Close ${t.title}`}
            onMouseDown={(e) => {
              e.stopPropagation();
              onClose(t.id);
            }}
          >
            <Icon name="close" size={11} />
          </button>
        </div>
      ))}
      <button className="tab add" aria-label="New local shell" title="New local shell" onClick={onNewLocal}>
        <Icon name="plus" size={13} />
      </button>
      <div className="tabs-hint">Drag tabs to reorder — drop on a pane or gutter to tile</div>
    </div>
  );
}
