import type { SessionNode } from "../types";

type Field = { label: string; key: keyof SessionNode; type?: "text" | "password" };

const CONNECTION_FIELDS: Field[] = [
  { label: "Name", key: "name" },
  { label: "Host/IP", key: "hostname" },
  { label: "Port", key: "port" },
  { label: "User", key: "username" },
  { label: "Password", key: "password", type: "password" },
  { label: "Private Key", key: "private_key" },
  { label: "Certificate", key: "certificate" },
  { label: "Login Dir", key: "login_dir" },
  { label: "Login Cmds", key: "login_cmds" },
  { label: "Cmd-line Args", key: "cmdline_args" },
  { label: "Env Color", key: "env_color" },
  { label: "Custom Id", key: "custom_id" },
  { label: "Custom Type", key: "custom_type" },
];

const CONTAINER_FIELDS: Field[] = [
  { label: "Name", key: "name" },
  { label: "Descr", key: "descr" },
];

/** The bottom-left "Configuration" panel — a WinForms PropertyGrid lookalike. */
export function ConfigPanel({
  node,
  onChange,
}: {
  node: SessionNode | null;
  onChange: (patch: Partial<SessionNode>) => void;
}) {
  const isContainer = node?.type === "Container";
  const fields = node ? (isContainer ? CONTAINER_FIELDS : CONNECTION_FIELDS) : [];

  return (
    <div className="panel panel-config">
      <div className="panel-title">
        <span>Configuration</span>
        <span className="spacer" />
        <button className="pbtn" title="Hide">✕</button>
      </div>

      <div className="prop-header">
        <span className="muted">Type</span>
        <select value={node ? node.type : "Connection"} disabled>
          <option value="Connection">Connection</option>
          <option value="Container">Container</option>
        </select>
        {node && isContainer && <span className="muted">folder</span>}
      </div>

      <div className="props">
        {!node && <div className="prop-empty">Select a session to edit its properties.</div>}
        {node &&
          fields.map((f) => (
            <div className="prop-row" key={String(f.key)}>
              <div className="prop-label" title={f.label}>{f.label}</div>
              <div className="prop-value">
                <input
                  type={f.type === "password" ? "password" : "text"}
                  value={String(node[f.key] ?? "")}
                  placeholder={f.label === "Private Key" ? "e.g. ~/.ssh/id_ed25519" : ""}
                  onChange={(e) => onChange({ [f.key]: e.target.value } as Partial<SessionNode>)}
                />
              </div>
            </div>
          ))}
        {node && !isContainer && (
          <div className="prop-note">
            Passwords are held in memory only and are <b>not</b> written to{" "}
            <span className="mono">connections.xml</span>. The encrypted vault (v0.2) will persist
            them securely.
          </div>
        )}
      </div>
    </div>
  );
}