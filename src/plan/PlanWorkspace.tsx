import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Segmented } from "../components/Field";
import { api, errorText } from "../lib/api";
import type { Palette, ThemeMode } from "../lib/theme";
import { MapView, modeColor, type Placing, type RiskLayer } from "./MapView";
import { PlanPanel } from "./PlanPanel";
import { SetupPanel } from "./SetupPanel";
import { Timeline } from "./Timeline";
import type { Plan, PlanScenario, Request, RiskLayers, World } from "./types";

const STORAGE_KEY = "lifeline.plan.v1";

function loadSaved(): PlanScenario | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as PlanScenario) : null;
  } catch {
    return null;
  }
}

function nextId(prefix: string, ids: string[]): string {
  const used = new Set(ids);
  for (let i = 1; ; i++) if (!used.has(`${prefix}${i}`)) return `${prefix}${i}`;
}

export interface WorkspaceHandle {
  scenario: PlanScenario | null;
  setScenario: (s: PlanScenario) => void;
  reset: () => void;
}

export function PlanWorkspace({
  palette,
  theme,
  onStatus,
  handle,
}: {
  palette: Palette;
  theme: ThemeMode;
  onStatus: (s: string) => void;
  handle: React.MutableRefObject<WorkspaceHandle | null>;
}) {
  const [scenario, setScenario] = useState<PlanScenario | null>(loadSaved);
  const [world, setWorld] = useState<World | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [layers, setLayers] = useState<RiskLayers | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [riskLayer, setRiskLayer] = useState<RiskLayer>("ground");
  const [placing, setPlacing] = useState<Placing>(null);

  const reset = useCallback(() => {
    api
      .examplePlan()
      .then((s) => {
        setScenario(s);
        setSelected(null);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  useEffect(() => {
    if (!scenario) reset();
  }, [scenario, reset]);

  handle.current = { scenario, setScenario: (s) => { setScenario(s); setSelected(null); }, reset };

  useEffect(() => {
    if (!scenario) return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(scenario));
    } catch {
      /* storage unavailable */
    }
  }, [scenario]);

  // the world only changes when its spec does
  const specKey = scenario ? JSON.stringify(scenario.world) : "";
  useEffect(() => {
    if (!scenario) return;
    let live = true;
    api
      .world(scenario.world)
      .then((w) => live && setWorld(w))
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [specKey]);

  // replan shortly after edits stop
  const key = useMemo(() => JSON.stringify(scenario), [scenario]);
  const generation = useRef(0);
  useEffect(() => {
    if (!scenario) return;
    const id = ++generation.current;
    const timer = setTimeout(async () => {
      try {
        const [p, l] = await Promise.all([api.plan(scenario), api.riskLayers(scenario)]);
        if (id !== generation.current) return;
        setPlan(p);
        setLayers(l);
        setError(null);
        onStatus(`Planned in ${Math.round(p.compute_ms)} ms`);
      } catch (e) {
        if (id === generation.current) setError(errorText(e));
      }
    }, 180);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPlacing(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const update = useCallback((fn: (s: PlanScenario) => PlanScenario) => setScenario((s) => (s ? fn(s) : s)), []);

  if (!scenario || !world) {
    return <div className="empty">{error ?? "Generating terrain…"}</div>;
  }

  const place = (x: number, y: number) => {
    if (placing === "request") {
      const id = nextId("R", scenario.requests.map((r) => r.id));
      const r: Request = { id, name: "New request", x, y, demand_kg: 200, priority: "high", due_h: 6, mode: "auto" };
      update((s) => ({ ...s, requests: [...s.requests, r] }));
      setSelected(id);
    } else if (placing === "site") {
      const id = nextId("S", scenario.risk.sites.map((q) => q.id));
      update((s) => ({
        ...s,
        risk: { ...s.risk, sites: [...s.risk.sites, { id, name: `Observed area ${id.slice(1)}`, x, y, height: 8, range: 3000, rate_per_min: 0.02 }] },
      }));
    } else if (placing === "hazard") {
      const id = nextId("H", scenario.risk.hazards.map((q) => q.id));
      const d = 500;
      update((s) => ({
        ...s,
        risk: {
          ...s.risk,
          hazards: [
            ...s.risk.hazards,
            { id, name: `Hazard ${id.slice(1)}`, polygon: [[x - d, y - d], [x + d, y - d], [x + d, y + d], [x - d, y + d]], rate_per_min: 0.2, impassable: false },
          ],
        },
      }));
    }
    setPlacing(null);
  };

  const move = (kind: "request" | "depot" | "site", id: string, x: number, y: number) => {
    if (kind === "depot") update((s) => ({ ...s, depot: { ...s.depot, x, y } }));
    else if (kind === "request") update((s) => ({ ...s, requests: s.requests.map((r) => (r.id === id ? { ...r, x, y } : r)) }));
    else update((s) => ({ ...s, risk: { ...s.risk, sites: s.risk.sites.map((q) => (q.id === id ? { ...q, x, y } : q)) } }));
  };

  return (
    <div className="plan-layout">
      <SetupPanel scenario={scenario} update={update} placing={placing} setPlacing={setPlacing} />
      <div className="plan-center">
        {error && (
          <div className="banner" role="alert">
            <strong>Cannot plan</strong>
            <span>{error}</span>
          </div>
        )}
        <div className="map-area">
          <MapView
            world={world}
            scenario={scenario}
            plan={plan}
            layers={layers}
            palette={palette}
            theme={theme}
            selected={selected}
            onSelect={setSelected}
            riskLayer={riskLayer}
            placing={placing}
            onPlace={place}
            onMove={move}
          />
          <div className="map-key">
            <div className="map-key-row">
              <span className="map-key-title">Exposure</span>
              <Segmented
                ariaLabel="Exposure layer"
                value={riskLayer}
                options={[
                  { value: "off", label: "Off" },
                  { value: "ground", label: "Ground" },
                  { value: "drone", label: "Drone" },
                  { value: "air", label: "Aircraft" },
                ]}
                onChange={setRiskLayer}
              />
            </div>
            <div className="legend">
              <span>
                <span className="line" style={{ borderColor: modeColor(palette, "truck") }} />
                Truck
              </span>
              <span>
                <span className="line dashed" style={{ borderColor: modeColor(palette, "drone") }} />
                Drone
              </span>
              <span>
                <span className="line dotted" style={{ borderColor: modeColor(palette, "airdrop") }} />
                Aircraft
              </span>
              <span>
                <span className="swatch" style={{ background: palette.critical, opacity: 0.45 }} />
                In view of a risk
              </span>
            </div>
          </div>
        </div>
        {plan && (
          <div className="timeline-area">
            <div className="block-head">
              <h2>Fleet timeline</h2>
              <span className="note">hours from now · solid outbound, faint return</span>
            </div>
            <Timeline plan={plan} scenario={scenario} palette={palette} selected={selected} onSelect={setSelected} />
          </div>
        )}
      </div>
      <PlanPanel
        scenario={scenario}
        plan={plan}
        palette={palette}
        selected={selected}
        onSelect={setSelected}
        adding={placing === "request"}
        onAddRequest={() => setPlacing(placing === "request" ? null : "request")}
        onChangeRequest={(id, patch) => update((s) => ({ ...s, requests: s.requests.map((r) => (r.id === id ? { ...r, ...patch } : r)) }))}
        onDeleteRequest={(id) => {
          update((s) => ({ ...s, requests: s.requests.filter((r) => r.id !== id) }));
          setSelected(null);
        }}
      />
    </div>
  );
}
