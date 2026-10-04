import type { Tab } from "../types";

export function TabStrip({
  tabs,
  activeId,
  onSelect,
  onClose,
  onNewLocal,
  onContextMenu,
}: {
  tabs: Tab[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onNewLocal: () => void;
  onContextMenu: (e: React.MouseEvent, id: string) => void;
}) {
  return (
    <div className="tabstrip">
      {tabs.map((t) => (
        <div
          key={t.id}
          className={`tab${t.id === activeId ? " active" : ""}`}
          onMouseDown={() => onSelect(t.id)}
          onContextMenu={(e) => onContextMenu(e, t.id)}
          title={`${t.title} — ${t.target}`}
        >
          <span className={`tab-dot ${t.exited ? "dead" : "pending"}`} />
          <span
            style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
          >
            {t.title}
          </span>
          <span
            className="tab-close"
            onMouseDown={(e) => {
              e.stopPropagation();
              onClose(t.id);
            }}
          >
            ✕
          </span>
        </div>
      ))}
      <button className="tab-add" onClick={onNewLocal} title="New local shell">＋</button>
    </div>
  );
}