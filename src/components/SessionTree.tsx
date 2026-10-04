import type { SessionNode } from "../types";

function FolderIcon({ open }: { open: boolean }) {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden>
      <path
        d="M1.5 3.5h4l1.2 1.5h7.8v7.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"
        fill={open ? "#f0c14b" : "#f5d98b"}
        stroke="#b58b2a"
        strokeWidth="0.8"
      />
      {open && <path d="M1.5 12.5l1.6-5h12l-1.7 5z" fill="#ffe9a8" stroke="#b58b2a" strokeWidth="0.8" />}
    </svg>
  );
}

function HostIcon({ resolved }: { resolved: boolean }) {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden>
      <rect x="1.5" y="2.5" width="13" height="8.5" rx="1" fill={resolved ? "#dbe6f2" : "#efefef"} stroke="#7d8b9a" strokeWidth="0.9" />
      <rect x="2.6" y="3.6" width="11.8" height="6.3" fill={resolved ? "#2f4f6f" : "#cfcfcf"} />
      <path d="M6 13.2h4l1 1.6H5z" fill="#9aa7b4" stroke="#7d8b9a" strokeWidth="0.7" />
      <path d="M8 11.2v2" stroke="#7d8b9a" strokeWidth="1" />
    </svg>
  );
}

export interface TreeCallbacks {
  onSelect: (path: number[]) => void;
  onToggle: (path: number[]) => void;
  onOpen: (path: number[]) => void;
  onContextMenu: (e: React.MouseEvent, path: number[]) => void;
}

function Row({
  node,
  path,
  depth,
  selectedKey,
  cb,
}: {
  node: SessionNode;
  path: number[];
  depth: number;
  selectedKey: string | null;
  cb: TreeCallbacks;
}) {
  const key = path.join(".");
  const isContainer = node.type === "Container";
  const resolved = isContainer || node.hostname !== "";
  const selected = selectedKey === key;

  return (
    <>
      <div
        className={`tree-row${selected ? " selected" : ""}`}
        style={{ paddingLeft: 4 + depth * 14 }}
        onMouseDown={() => cb.onSelect(path)}
        onDoubleClick={() => (isContainer ? cb.onToggle(path) : cb.onOpen(path))}
        onContextMenu={(e) => cb.onContextMenu(e, path)}
        title={isContainer ? node.name : `${node.name} — ${node.username || "?"}@${node.hostname || "unresolved"}:${node.port || "22"}`}
      >
        <span
          className={`caret${isContainer ? "" : " spacer"}`}
          onMouseDown={(e) => {
            e.stopPropagation();
            if (isContainer) cb.onToggle(path);
          }}
        >
          {isContainer ? (node.expanded ? "▼" : "▶") : ""}
        </span>
        <span className="ico">{isContainer ? <FolderIcon open={node.expanded} /> : <HostIcon resolved={resolved} />}</span>
        <span className={`label${resolved ? "" : " unresolved"}`}>{node.name}</span>
      </div>
      {isContainer &&
        node.expanded &&
        node.children.map((c, i) => (
          <Row key={i} node={c} path={[...path, i]} depth={depth + 1} selectedKey={selectedKey} cb={cb} />
        ))}
    </>
  );
}

export function SessionTree({
  tree,
  selectedPath,
  cb,
}: {
  tree: SessionNode[];
  selectedPath: number[] | null;
  cb: TreeCallbacks;
}) {
  const selectedKey = selectedPath ? selectedPath.join(".") : null;
  return (
    <div className="tree">
      {tree.map((n, i) => (
        <Row key={i} node={n} path={[i]} depth={0} selectedKey={selectedKey} cb={cb} />
      ))}
      {tree.length === 0 && <div className="prop-empty">No sessions. Use File → Import connections…</div>}
    </div>
  );
}