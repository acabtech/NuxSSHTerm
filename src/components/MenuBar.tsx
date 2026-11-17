import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

export interface MenuDef {
  label: string;
  items: Array<
    | { kind: "divider" }
    | { kind: "item"; label: string; hint?: string; disabled?: boolean; onClick: () => void }
  >;
}

/** Modern menu bar (v0.5.0): brand + dropdown menus + current-page badge. */
export function MenuBar({
  menus,
  page,
}: {
  menus: MenuDef[];
  page: "workspace" | "settings";
}) {
  const [open, setOpen] = useState<number | null>(null);
  const [pos, setPos] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open === null) return;
    const close = () => setOpen(null);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const menu = open !== null ? menus[open] : null;

  return (
    <div className="menubar" role="menubar" aria-label="Application menus" ref={barRef}>
      <div className="brand">
        <Icon name="terminal" size={15} /> NuxSSHTerm
      </div>

      {menus.map((m, i) => (
        <div
          key={m.label}
          className={`menu-root${open === i ? " open" : ""}`}
          onMouseDown={(e) => {
            e.stopPropagation();
            const r = (e.target as HTMLElement).getBoundingClientRect();
            setPos({ x: r.left, y: r.bottom });
            setOpen(open === i ? null : i);
          }}
          onMouseEnter={(e) => {
            if (open !== null && open !== i) {
              const btn = (e.currentTarget as HTMLElement).querySelector(".menu-btn");
              if (btn) {
                const r = btn.getBoundingClientRect();
                setPos({ x: r.left, y: r.bottom });
              }
              setOpen(i);
            }
          }}
        >
          <button className="menu-btn" aria-haspopup="menu" aria-expanded={open === i}>
            {m.label}
          </button>
        </div>
      ))}

      <div className="menu-spacer" />
      <span className="page-badge">{page === "settings" ? "Settings" : "Workspace"}</span>

      {menu && (
        <div
          className="menu-pop"
          style={{ left: pos.x, top: pos.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {menu.items.map((it, idx) =>
            it.kind === "divider" ? (
              <div className="menu-sep" key={`d${idx}`} />
            ) : (
              <div
                key={it.label}
                className={`menu-item${it.disabled ? " disabled" : ""}`}
                onClick={() => {
                  if (it.disabled) return;
                  setOpen(null);
                  it.onClick();
                }}
              >
                <span>{it.label}</span>
                {it.hint && <span className="hint">{it.hint}</span>}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
