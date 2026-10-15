import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorText } from "../lib/api";
import { ScenarioPanel } from "./ScenarioPanel";
import { fmt } from "../lib/format";
import type { Palette } from "../lib/theme";
import { isDropScenario, type DropReport, type MonteCarloReport, type NumericsReport, type ProfileRow, type Scenario } from "./types";
import { AtmosphereView } from "./views/AtmosphereView";
import { DispersionView } from "./views/DispersionView";
import { DropView } from "./views/DropView";
import { NumericsView } from "./views/NumericsView";
import { ReleaseView } from "./views/ReleaseView";

type Tab = "drop" | "plan" | "dispersion" | "numerics" | "atmosphere";
const TABS: { id: Tab; label: string }[] = [
  { id: "drop", label: "3D drop" },
  { id: "plan", label: "Release plan" },
  { id: "dispersion", label: "Dispersion" },
  { id: "numerics", label: "Numerics" },
  { id: "atmosphere", label: "Atmosphere" },
];

const STORAGE_KEY = "lifeline.airdrop.v1";

function loadSaved(): Scenario | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const s: unknown = raw ? JSON.parse(raw) : null;
    return isDropScenario(s) ? s : null;
  } catch {
    return null;
  }
}

/** Run `fn` `delay` ms after `deps` stop changing; ignore results of superseded runs. */
function useDebouncedTask(enabled: boolean, deps: unknown[], delay: number, fn: (isCurrent: () => boolean) => Promise<void>) {
  const gen = useRef(0);
  useEffect(() => {
    if (!enabled) return;
    const id = ++gen.current;
    const timer = setTimeout(() => void fn(() => id === gen.current), delay);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, ...deps]);
}

export interface LabHandle {
  scenario: Scenario | null;
  setScenario: (s: Scenario) => void;
  reset: () => void;
}

export function AirdropLab({
  palette,
  onStatus,
  handle,
  initial,
}: {
  palette: Palette;
  onStatus: (s: string) => void;
  handle: React.MutableRefObject<LabHandle | null>;
  /** scenario to start from instead of the saved one (a file opened before the lab was first shown) */
  initial?: Scenario | null;
}) {
  const [tab, setTab] = useState<Tab>("drop");
  const [scenario, setScenario] = useState<Scenario | null>(() => initial ?? loadSaved());
  const [drop, setDrop] = useState<DropReport | null>(null);
  const [mc, setMc] = useState<{ key: string; report: MonteCarloReport } | null>(null);
  const [mcBusy, setMcBusy] = useState(false);
  const [numerics, setNumerics] = useState<NumericsReport | null>(null);
  const [compareDt, setCompareDt] = useState(0.5);
  const [profile, setProfile] = useState<ProfileRow[] | null>(null);
  const [zMax, setZMax] = useState(11000);
  const [error, setError] = useState<string | null>(null);

  const reset = useCallback(() => {
    api.defaultScenario().then(setScenario).catch((e) => setError(errorText(e)));
  }, []);

  useEffect(() => {
    if (!scenario) reset();
  }, [scenario, reset]);

  handle.current = { scenario, setScenario, reset };

  useEffect(() => {
    if (!scenario) return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(scenario));
    } catch {
      /* ignore */
    }
  }, [scenario]);

  const key = useMemo(() => JSON.stringify(scenario), [scenario]);
  const update = useCallback((fn: (s: Scenario) => Scenario) => setScenario((s) => (s ? fn(s) : s)), []);

  useDebouncedTask(!!scenario, [key], 80, async (current) => {
    try {
      const r = await api.drop(scenario!);
      if (!current()) return;
      setDrop(r);
      setError(null);
    } catch (e) {
      if (current()) setError(errorText(e));
    }
  });

  const runMc = useCallback(async () => {
    if (!scenario) return;
    const k = JSON.stringify(scenario);
    setMcBusy(true);
    try {
      const r = await api.monteCarlo(scenario);
      setMc({ key: k, report: r });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setMcBusy(false);
    }
  }, [scenario]);

  useDebouncedTask(!!scenario && tab === "dispersion", [key], 400, async (current) => {
    if (mc?.key === key || !current()) return;
    await runMc();
  });

  useDebouncedTask(!!scenario && tab === "numerics", [key, compareDt], 200, async (current) => {
    try {
      const r = await api.numerics(scenario!, compareDt);
      if (current()) setNumerics(r);
    } catch (e) {
      if (current()) setError(errorText(e));
    }
  });

  useDebouncedTask(!!scenario && tab === "atmosphere", [key, zMax], 100, async (current) => {
    try {
      const r = await api.profile(scenario!, zMax, 161);
      if (current()) setProfile(r);
    } catch (e) {
      if (current()) setError(errorText(e));
    }
  });

  const exportImpacts = async () => {
    if (!mc) return;
    const r = mc.report.result;
    const rows = r.impacts.map(([e, n], i) => `${e.toFixed(3)},${n.toFixed(3)},${r.times_of_fall[i].toFixed(3)}`);
    try {
      const path = await api.saveText("impacts.csv", "csv", ["east_m,north_m,time_of_fall_s", ...rows].join("\n"));
      if (path) onStatus(`Saved ${path}`);
    } catch (e) {
      setError(errorText(e));
    }
  };

  useEffect(() => {
    if (drop && scenario) onStatus(`${scenario.integrator.method.toUpperCase()}, dt ${scenario.integrator.dt} s, solved in ${fmt(drop.compute_ms, 1)} ms`);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drop]);

  const mcCurrent = mc && mc.key === key ? mc.report : null;
  const rel = drop?.release;

  return (
    <div className="lab-layout">
      <aside className="sidebar">{scenario && <ScenarioPanel scenario={scenario} update={update} />}</aside>

      <main className="main">
        <nav className="subtabs" role="tablist" aria-label="Airdrop lab views">
          {TABS.map((t) => (
            <button key={t.id} role="tab" className="subtab" aria-selected={tab === t.id} onClick={() => setTab(t.id)}>
              {t.label}
            </button>
          ))}
        </nav>
        {rel && (
          <div className="kpis">
            <Kpi label="Time of fall" value={`${fmt(rel.time_of_fall, 1)} s`} sub={`${fmt(rel.impact_vertical_speed, 1)} m/s descent at impact`} />
            <Kpi label="Release before target" value={`${fmt(rel.release_distance_before_target, 0)} m`} sub={`green light ${fmt(rel.release_lead_time, 1)} s early`} />
            <Kpi
              label="Crosswind drift"
              value={`${fmt(Math.abs(rel.cross_track_drift), 0)} m`}
              sub={rel.cross_track_drift >= 0 ? "to the right of track" : "to the left of track"}
            />
            <Kpi label="Impact speed" value={`${fmt(rel.impact_speed, 1)} m/s`} sub={`${fmt(0.5 * (scenario?.payload.mass ?? 0) * rel.impact_speed ** 2 / 1000, 1)} kJ`} />
            <Kpi
              label="CEP"
              value={mcCurrent ? `${fmt(mcCurrent.result.cep, 0)} m` : "—"}
              sub={mcCurrent ? `${fmt(mcCurrent.inside_drop_zone * 100, 1)} % inside drop zone` : "run Dispersion"}
            />
          </div>
        )}
        {error && (
          <div className="banner" role="alert">
            <strong>Error</strong>
            <span>{error}</span>
          </div>
        )}
        <section className="view">
          {!scenario || !drop ? (
            <div className="empty">Solving…</div>
          ) : tab === "drop" ? (
            <DropView scenario={scenario} report={drop} mc={mcCurrent} palette={palette} />
          ) : tab === "plan" ? (
            <ReleaseView scenario={scenario} report={drop} palette={palette} />
          ) : tab === "dispersion" ? (
            <DispersionView
              scenario={scenario}
              report={drop}
              mc={mcCurrent ?? mc?.report ?? null}
              busy={mcBusy}
              palette={palette}
              onRun={runMc}
              onExport={exportImpacts}
            />
          ) : tab === "numerics" ? (
            <NumericsView report={numerics} dt={compareDt} setDt={setCompareDt} palette={palette} />
          ) : (
            <AtmosphereView scenario={scenario} rows={profile} zMax={zMax} setZMax={setZMax} palette={palette} />
          )}
        </section>
      </main>
    </div>
  );
}

function Kpi({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
      <div className="sub">{sub}</div>
    </div>
  );
}
