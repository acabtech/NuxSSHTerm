import type { Tab } from "../types";

export function StatusBar({
  tab,
  configDir,
  sessionCount,
  notice,
}: {
  tab: Tab | null;
  configDir: string;
  sessionCount: number;
  notice: string;
}) {
  return (
    <div className="statusbar">
      <div className={`cell${tab && !tab.exited ? " ok" : ""}`}>
        {tab ? (tab.exited ? "disconnected" : "connected") : "ready"}
      </div>
      <div className="cell">{tab ? tab.target : "no active session"}</div>
      <div className="cell grow">{notice}</div>
      <div className="cell">{sessionCount} hosts</div>
      <div className="cell mono" title={configDir}>
        {configDir || "…"}
      </div>
    </div>
  );
}