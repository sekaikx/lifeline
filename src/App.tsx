import { useEffect, useRef, useState } from "react";
import { AirdropLab, type LabHandle } from "./airdrop/AirdropLab";
import { isDropScenario, type Scenario as DropScenario } from "./airdrop/types";
import { AssistantWorkspace } from "./ai/AssistantWorkspace";
import { IntelWorkspace } from "./intel/IntelWorkspace";
import { defaultSatellite, type SatelliteChoice } from "./intel/types";
import { api, errorText, hasBackend } from "./lib/api";
import { applyTheme, initialTheme, palettes, type ThemeMode } from "./lib/theme";
import { PlanWorkspace, type PlanState, type SupplyView, type WorkspaceHandle } from "./plan/PlanWorkspace";
import { isPlanScenario } from "./plan/types";
import { SupplyWorkspace } from "./supply/SupplyWorkspace";

const NO_SUPPLY: SupplyView = { catalogue: [], available: {}, shortfalls: {} };

type Workspace = "plan" | "intel" | "supply" | "assistant" | "lab";

export default function App() {
  const [theme, setTheme] = useState<ThemeMode>(initialTheme);
  const [workspace, setWorkspace] = useState<Workspace>("plan");
  const [status, setStatus] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const planHandle = useRef<WorkspaceHandle | null>(null);
  const labHandle = useRef<LabHandle | null>(null);
  const [labInitial, setLabInitial] = useState<DropScenario | null>(null);
  const [planState, setPlanState] = useState<PlanState | null>(null);
  const [satellite, setSatellite] = useState<SatelliteChoice>(defaultSatellite);
  const [supply, setSupply] = useState<SupplyView>(NO_SUPPLY);
  const [scenePng, setScenePng] = useState<string | null>(null);
  const [sceneBounds, setSceneBounds] = useState<[number, number, number, number] | null>(null);
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

  // drop the intel scene when the planning area changes
  const areaKey = planState?.area
    ? `${planState.area.name}:${planState.area.lat}:${planState.area.lon}:${planState.area.size_m}`
    : "";
  useEffect(() => {
    setScenePng(null);
    setSceneBounds(null);
  }, [areaKey]);

  const flash = (msg: string) => {
    setNotice(msg);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 3500);
  };

  if (!hasBackend()) {
    return (
      <div className="empty">
        <div>
          <img className="empty-brand" src="/app-icon.svg" alt="" width={64} height={64} />
          <h1 className="empty-title">Lifeline</h1>
          <p>
            The planning engine runs in Rust inside the desktop app. Start it with <code>npm run tauri dev</code>.
          </p>
        </div>
      </div>
    );
  }

  const save = async () => {
    const s = workspace === "lab" ? labHandle.current?.scenario : planHandle.current?.scenario;
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

  const reset = () => {
    if (workspace === "lab") labHandle.current?.reset();
    else planHandle.current?.reset();
  };

  const tabs: { id: Workspace; label: string }[] = [
    { id: "plan", label: "Plan" },
    { id: "intel", label: "Area intel" },
    { id: "supply", label: "Supply" },
    { id: "assistant", label: "Assistant" },
    { id: "lab", label: "Airdrop lab" },
  ];

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <img src="/app-icon.svg" alt="" />
          Lifeline
        </div>
        <nav className="workspaces" role="tablist" aria-label="Workspace">
          {tabs.map((t) => (
            <button key={t.id} role="tab" aria-selected={workspace === t.id} onClick={() => setWorkspace(t.id)}>
              {t.label}
            </button>
          ))}
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
      <div className="workspace" hidden={workspace !== "plan"}>
        <PlanWorkspace
          palette={palette}
          theme={theme}
          onStatus={setStatus}
          handle={planHandle}
          satellite={satellite}
          supply={supply}
          onState={setPlanState}
        />
      </div>
      <div className="workspace" hidden={workspace !== "intel"}>
        {(workspace === "intel" || planState) && (
          <IntelWorkspace
            area={planState?.area ?? null}
            satellite={satellite}
            onSatellite={setSatellite}
            onScene={(png, bounds) => {
              setScenePng(png);
              setSceneBounds(bounds);
            }}
            onStatus={setStatus}
          />
        )}
      </div>
      <div className="workspace" hidden={workspace !== "supply"}>
        {(workspace === "supply" || supply.catalogue.length > 0) && (
          <SupplyWorkspace
            scenario={planState?.scenario ?? null}
            plan={planState?.plan ?? null}
            onSupply={setSupply}
            onStatus={setStatus}
          />
        )}
      </div>
      <div className="workspace" hidden={workspace !== "assistant"}>
        {(workspace === "assistant" || scenePng) && (
          <AssistantWorkspace
            planState={planState}
            planHandle={planHandle}
            catalogue={supply.catalogue}
            scenePng={scenePng}
            sceneBounds={sceneBounds}
            onStatus={setStatus}
          />
        )}
      </div>
      <div className="workspace" hidden={workspace !== "lab"}>
        {workspace === "lab" || labHandle.current ? (
          <AirdropLab palette={palette} onStatus={setStatus} handle={labHandle} initial={labInitial} />
        ) : null}
      </div>
    </div>
  );
}
