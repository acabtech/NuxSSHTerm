// Shared types mirroring the Rust `Node` model (WinSSHTerm session XML).

export type NodeType = "Container" | "Connection";

export interface SessionNode {
  name: string;
  type: NodeType;
  expanded: boolean;
  children: SessionNode[];

  // WinSSHTerm connection attributes
  descr: string;
  username: string;
  password: string;
  private_key: string;
  hostname: string;
  port: string;
  certificate: string;
  launch_tool: string;
  x11: string;
  cf_prot: string;
  proxy_enabled: boolean;
  proxy_type: string;
  proxy_host: string;
  proxy_port: string;
  proxy_user: string;
  proxy_telnet_cmd: string;

  // UI-local properties (Configuration panel: Login Dir, Login Cmds, …)
  login_dir: string;
  login_cmds: string;
  cmdline_args: string;
  env_color: string;
  custom_id: string;
  custom_type: string;
}

export interface LaunchSpec {
  host: string;
  port: number;
  username: string;
  private_key: string;
  x11: boolean;
  forward_agent: boolean;
  extra_args: string[];
  proxy_enabled: boolean;
  proxy_type: string;
  proxy_host: string;
  proxy_port: string;
  proxy_telnet_cmd: string;
}

export interface Tab {
  /** unique tab id, also the pty session id */
  id: string;
  title: string;
  /** "terminal" | "commander" */
  kind: "terminal" | "commander";
  spec: LaunchSpec;
  target: string;
  exited: boolean;
}

export function emptyNode(name: string, type: NodeType): SessionNode {
  return {
    name,
    type,
    expanded: type === "Container",
    children: [],
    descr: "",
    username: "",
    password: "",
    private_key: "",
    hostname: "",
    port: type === "Connection" ? "22" : "",
    certificate: "",
    launch_tool: "",
    x11: "don't forward",
    cf_prot: type === "Connection" ? "sftp" : "",
    proxy_enabled: false,
    proxy_type: "",
    proxy_host: "",
    proxy_port: "",
    proxy_user: "",
    proxy_telnet_cmd: "",
    login_dir: "",
    login_cmds: "",
    cmdline_args: "",
    env_color: "",
    custom_id: "",
    custom_type: "",
  };
}

export function specFromNode(n: SessionNode): LaunchSpec {
  return {
    host: n.hostname,
    port: parseInt(n.port || "22", 10) || 22,
    username: n.username,
    private_key: n.private_key,
    x11: n.x11 !== "" && !n.x11.toLowerCase().startsWith("don't"),
    forward_agent: false,
    extra_args: n.cmdline_args ? n.cmdline_args.split(/\s+/).filter(Boolean) : [],
    proxy_enabled: n.proxy_enabled,
    proxy_type: n.proxy_type,
    proxy_host: n.proxy_host,
    proxy_port: n.proxy_port,
    proxy_telnet_cmd: n.proxy_telnet_cmd,
  };
}

export function targetOf(n: SessionNode): string {
  const user = n.username || "?";
  return `${user}@${n.hostname || "?"}:${n.port || "22"}`;
}
