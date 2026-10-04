import { useEffect, useRef, useState } from "react";

export interface MenuDef {
  label: string;
  items: Array<
    | { kind: "divider" }
    | { kind: "item"; label: string; hint?: string; disabled?: boolean; onClick: () => void }
  >;
}

/** WinSSHTerm's menu bar plus the right-aligned quick strip (Scripts | Paste | Visible | Con | None). */
export function MenuBar({
  menus,
  quick,
}: {
  menus: MenuDef[];
  quick: { paste: () => void; target: string };
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
    <div className="topbar">
      <div className="menubar" ref={barRef}>
        {menus.map((m, i) => (
          <div
            key={m.label}
            className={`menubar-item${open === i ? " open" : ""}`}
            onMouseDown={(e) => {
              e.stopPropagation();
              const r = (e.target as HTMLElement).getBoundingClientRect();
              setPos({ x: r.left, y: r.bottom });
              setOpen(open === i ? null : i);
            }}
            onMouseEnter={() => {
              if (open !== null && open !== i) {
                const el = barRef.current?.children[i] as HTMLElement | undefined;
                if (el) {
                  const r = el.getBoundingClientRect();
                  setPos({ x: r.left, y: r.bottom });
                }
                setOpen(i);
              }
            }}
          >
            {m.label}
          </div>
        ))}
      </div>

      <div className="quickstrip" onMouseDown={(e) => e.stopPropagation()}>
        <button className="qs-btn" title="Launch scripts (v0.3)">Scripts</button>
        <button className="qs-btn" title="Paste clipboard into the active terminal" onClick={quick.paste}>
          Paste
        </button>
        <button className="qs-btn" title="Quick-launch bar visibility (v0.2)">Visible</button>
        <button className="qs-btn" title="Configuration panel">Con</button>
        <button className="qs-btn" title="No quick-launch bar">None</button>
      </div>

      {menu && (
        <div
          className="menu-pop"
          style={{ left: pos.x, top: pos.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {menu.items.map((it, idx) =>
            it.kind === "divider" ? (
              <div className="menu-divider" key={`d${idx}`} />
            ) : (
              <div
                key={it.label}
                className={`menu-entry${it.disabled ? " disabled" : ""}`}
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