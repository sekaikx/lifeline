import { useEffect, useRef, useState } from "react";
import { AirdropLab, type LabHandle } from "./airdrop/AirdropLab";
import { isDropScenario, type Scenario as DropScenario } from "./airdrop/types";
import { api, errorText, hasBackend } from "./lib/api";
import { applyTheme, initialTheme, palettes, type ThemeMode } from "./lib/theme";
import { defaultSatellite } from "./intel/types";
import { PlanWorkspace, type WorkspaceHandle } from "./plan/PlanWorkspace";

const NO_SUPPLY = { catalogue: [], available: {}, shortfalls: {} };
import { isPlanScenario } from "./plan/types";

type Workspace = "plan" | "lab";

export default function App() {
  const [theme, setTheme] = useState<ThemeMode>(initialTheme);
  const [workspace, setWorkspace] = useState<Workspace>("plan");
  const [status, setStatus] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const planHandle = useRef<WorkspaceHandle | null>(null);
  const labHandle = useRef<LabHandle | null>(null);
  // a lab scenario opened before the lab was first shown
  const [labInitial, setLabInitial] = useState<DropScenario | null>(null);
  const noticeTimer = useRef(0);
  const palette = palettes[theme];

  useEffect(() => {
    applyTheme(theme);
    try {
      localStorage.setItem("lifeline.theme", theme);
    } catch {
      /* storage unavailable */
    }
  }, [theme]);

  const flash = (msg: string) => {
    setNotice(msg);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 3500);
  };

  if (!hasBackend()) {
    return (
      <div className="empty">
        <div>
          <h1 className="empty-title">Lifeline</h1>
          <p>
            The planning engine runs in Rust inside the desktop app. Start it with <code>npm run tauri dev</code>.
          </p>
        </div>
      </div>
    );
  }

  const save = async () => {
    const s = workspace === "plan" ? planHandle.current?.scenario : labHandle.current?.scenario;
    if (!s) return;
    const base = (s.name ?? "").replace(/[^\w-]+/g, "-").toLowerCase() || "scenario";
    try {
      const path = await api.saveText(`${base}.json`, "json", JSON.stringify(s, null, 2));
      if (path) flash(`Saved ${path}`);
    } catch (e) {
      flash(errorText(e));
    }
  };

  const open = async () => {
    try {
      const text = await api.openText();
      if (!text) return;
      const data: unknown = JSON.parse(text);
      if (isPlanScenario(data)) {
        planHandle.current?.setScenario(data);
        setWorkspace("plan");
      } else if (isDropScenario(data)) {
        if (labHandle.current) labHandle.current.setScenario(data);
        else setLabInitial(data);
        setWorkspace("lab");
      } else {
        throw new Error("Not a Lifeline scenario file, or one with missing sections");
      }
      flash(`Opened ${data.name || "scenario"}`);
    } catch (e) {
      flash(errorText(e));
    }
  };

  const reset = () => (workspace === "plan" ? planHandle.current?.reset() : labHandle.current?.reset());

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <img src="/app-icon.svg" alt="" />
          Lifeline
        </div>
        <nav className="workspaces" role="tablist" aria-label="Workspace">
          <button role="tab" aria-selected={workspace === "plan"} onClick={() => setWorkspace("plan")}>
            Plan
          </button>
          <button role="tab" aria-selected={workspace === "lab"} onClick={() => setWorkspace("lab")}>
            Airdrop lab
          </button>
        </nav>
        <div className="spacer" />
        <span className="status num" aria-live="polite">
          {notice ?? status}
        </span>
        <div className="actions">
          <button className="btn quiet" onClick={open}>
            Open
          </button>
          <button className="btn quiet" onClick={save}>
            Save
          </button>
          <button className="btn quiet" onClick={reset}>
            Reset
          </button>
          <button className="btn quiet" onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>
            {theme === "dark" ? "Light theme" : "Dark theme"}
          </button>
        </div>
      </header>
      {/* both stay mounted so switching keeps each workspace's state */}
      <div className="workspace" hidden={workspace !== "plan"}>
        <PlanWorkspace palette={palette} theme={theme} onStatus={setStatus} handle={planHandle} satellite={defaultSatellite} supply={NO_SUPPLY} onState={() => {}} />
      </div>
      <div className="workspace" hidden={workspace !== "lab"}>
        {workspace === "lab" || labHandle.current ? <AirdropLab palette={palette} onStatus={setStatus} handle={labHandle} initial={labInitial} /> : null}
      </div>
    </div>
  );
}
