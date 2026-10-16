import { useEffect, useMemo, useState } from "react";
import { NumberField, Section, SelectField } from "../components/Field";
import { api, errorText } from "../lib/api";
import { fmtPct } from "../lib/format";
import type { Area } from "../plan/types";
import { ImageViewer } from "./ImageViewer";
import {
  PRODUCTS,
  SENSORS,
  defaultSatellite,
  productFits,
  type ChangeReport,
  type Product,
  type SatelliteChoice,
  type SceneImage,
  type SceneInfo,
  type Sensor,
} from "./types";

function toUnix(date: string): number | null {
  if (!date) return null;
  const t = Date.parse(`${date}T12:00:00Z`);
  return Number.isFinite(t) ? Math.round(t / 1000) : null;
}

function dataUrl(b64: string) {
  return `data:image/png;base64,${b64}`;
}

export function IntelWorkspace({
  area,
  satellite,
  onSatellite,
  onScene,
  onStatus,
}: {
  area: Area | null;
  satellite: SatelliteChoice;
  onSatellite: (s: SatelliteChoice) => void;
  /** Latest rendered scene PNG (no data: prefix) for the assistant. */
  onScene: (png: string | null, bounds: [number, number, number, number] | null) => void;
  onStatus: (s: string) => void;
}) {
  const [sensor, setSensor] = useState<Sensor>(satellite.query.sensor);
  const [product, setProduct] = useState<Product>(satellite.product);
  const [asOf, setAsOf] = useState("");
  const [windowDays, setWindowDays] = useState(satellite.query.window_days);
  const [maxCloud, setMaxCloud] = useState(satellite.query.max_cloud);
  const [scenes, setScenes] = useState<SceneInfo[]>([]);
  const [picked, setPicked] = useState<string | null>(null);
  const [image, setImage] = useState<SceneImage | null>(null);
  const [compareDate, setCompareDate] = useState("");
  const [change, setChange] = useState<ChangeReport | null>(null);
  const [grid, setGrid] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const products = useMemo(() => PRODUCTS.filter((p) => productFits(p.value, sensor)), [sensor]);

  useEffect(() => {
    if (!productFits(product, sensor)) setProduct(products[0]?.value ?? "true_color");
  }, [sensor, product, products]);

  const list = async () => {
    if (!area) return;
    setBusy("Listing scenes…");
    setError(null);
    onStatus(`Listing ${sensor} scenes`);
    const to = toUnix(asOf) ?? Math.round(Date.now() / 1000);
    const from = to - Math.max(windowDays, 1) * 86400;
    try {
      const list = await api.listScenes(area, sensor, from, to, sensor === "sentinel1" ? null : maxCloud, 80);
      setScenes(list);
      setPicked(list[0]?.id ?? null);
      if (!list.length) setError("No scenes in that window.");
    } catch (e) {
      setError(errorText(e));
      setScenes([]);
    } finally {
      setBusy(null);
      onStatus("");
    }
  };

  const loadImage = async (sceneId: string | null) => {
    if (!area) return;
    setBusy("Rendering scene…");
    setError(null);
    setChange(null);
    onStatus("Rendering satellite product");
    const query = {
      sensor,
      scene: sceneId,
      as_of: sceneId ? null : toUnix(asOf),
      window_days: windowDays,
      max_cloud: maxCloud,
    };
    try {
      const img = await api.sceneImage(area, query, product);
      setImage(img);
      onScene(img.png_base64, img.bounds);
      onSatellite({ query, product });
    } catch (e) {
      setError(errorText(e));
      setImage(null);
      onScene(null, null);
    } finally {
      setBusy(null);
      onStatus("");
    }
  };

  const runCompare = async () => {
    if (!area || !compareDate || !asOf) {
      setError("Set both As of and Compare to dates.");
      return;
    }
    setBusy("Comparing dates…");
    setError(null);
    onStatus("Comparing two looks");
    const before = {
      sensor,
      scene: null as string | null,
      as_of: toUnix(compareDate),
      window_days: windowDays,
      max_cloud: maxCloud,
    };
    const after = {
      sensor,
      scene: null as string | null,
      as_of: toUnix(asOf),
      window_days: windowDays,
      max_cloud: maxCloud,
    };
    try {
      const report = await api.compareScenes(area, before, after);
      setChange(report);
      setImage(null);
      onScene(report.png_base64, report.bounds);
    } catch (e) {
      setError(errorText(e));
      setChange(null);
    } finally {
      setBusy(null);
      onStatus("");
    }
  };

  const useOnPlan = () => {
    if (!image) return;
    onSatellite({
      query: {
        sensor: image.sensor,
        scene: image.scenes[0]?.id ?? null,
        as_of: toUnix(image.scenes[0]?.date ?? asOf),
        window_days: windowDays,
        max_cloud: maxCloud,
      },
      product: image.product,
    });
    onStatus("Plan map will use this satellite base");
  };

  if (!area) {
    return (
      <div className="empty">
        <div>
          <h1 className="empty-title">Area intel</h1>
          <p>Load a real place in Plan first (for example Dadu or Gorkha). Scenes are listed for that area.</p>
        </div>
      </div>
    );
  }

  const src = change ? dataUrl(change.png_base64) : image ? dataUrl(image.png_base64) : null;
  const bounds = change?.bounds ?? image?.bounds ?? null;
  const stats = change?.stats;
  const sceneStats = image?.stats;

  return (
    <div className="split-workspace">
      <aside className="setup">
        <Section title="Area">
          <p className="note">
            {area.name} · {area.size_m / 1000} km · {area.lat.toFixed(3)}°, {area.lon.toFixed(3)}°
          </p>
        </Section>
        <Section title="Sensor">
          <SelectField
            label="Mission"
            value={sensor}
            options={SENSORS.map((s) => ({ value: s.value, label: `${s.label} (${s.since}–)` }))}
            onChange={setSensor}
          />
          <p className="note">{SENSORS.find((s) => s.value === sensor)?.note}</p>
          <SelectField
            label="Product"
            value={product}
            options={products.map((p) => ({ value: p.value, label: p.label }))}
            onChange={setProduct}
          />
          <p className="note">{PRODUCTS.find((p) => p.value === product)?.help}</p>
          <label className="field">
            <span>As of</span>
            <input className="text-input" type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
          </label>
          <NumberField label="Window" unit="days" min={1} max={365} integer step={1} value={windowDays} onChange={setWindowDays} />
          {sensor !== "sentinel1" && (
            <NumberField label="Max cloud" unit="%" min={0} max={100} integer step={5} value={maxCloud} onChange={setMaxCloud} />
          )}
          <div className="btn-row">
            <button className="btn" disabled={!!busy} onClick={list}>
              List scenes
            </button>
            <button className="btn" disabled={!!busy} onClick={() => loadImage(null)}>
              Mosaic to date
            </button>
          </div>
        </Section>
        <Section title="Scenes" open={scenes.length > 0}>
          {scenes.length === 0 ? (
            <p className="note">List scenes for the window, or render a clear-sky mosaic up to the date.</p>
          ) : (
            <ul className="scene-list">
              {scenes.map((s) => (
                <li key={s.id}>
                  <button
                    type="button"
                    className="scene-row"
                    aria-pressed={picked === s.id}
                    disabled={!!busy}
                    onClick={() => {
                      setPicked(s.id);
                      void loadImage(s.id);
                    }}
                  >
                    <span className="num">{s.date}</span>
                    <span className="muted">{s.platform}</span>
                    <span className="num">{s.cloud == null ? "radar" : `${s.cloud.toFixed(0)}% cloud`}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Section>
        <Section title="Compare" open={false}>
          <label className="field">
            <span>Before</span>
            <input className="text-input" type="date" value={compareDate} onChange={(e) => setCompareDate(e.target.value)} />
          </label>
          <p className="note">Uses mosaics up to Before and As of. Good for flood, vegetation loss and burns.</p>
          <button className="btn" disabled={!!busy} onClick={runCompare}>
            Compare dates
          </button>
          {change && <p className="note">{change.method}</p>}
        </Section>
        <Section title="Display">
          <label className="toggle">
            <input type="checkbox" checked={grid} onChange={(e) => setGrid(e.target.checked)} />
            A1–H8 grid
          </label>
          <button className="btn" disabled={!image} onClick={useOnPlan}>
            Use on Plan map
          </button>
          <button
            className="btn quiet"
            onClick={() => {
              onSatellite(defaultSatellite);
              setImage(null);
              setChange(null);
            }}
          >
            Reset to default
          </button>
        </Section>
        {busy && <p className="note">{busy}</p>}
        {error && <p className="form-error">{error}</p>}
      </aside>
      <div className="intel-main">
        <ImageViewer src={src} bounds={bounds} grid={grid} markers={[]}>
          {!src && <div className="iv-empty">List a scene or render a mosaic to see imagery here.</div>}
        </ImageViewer>
        {(sceneStats || stats) && (
          <div className="intel-stats num">
            {sceneStats && (
              <>
                <span>Water {fmtPct(sceneStats.water_fraction, 1)}</span>
                <span>Veg {fmtPct(sceneStats.dense_vegetation_fraction + sceneStats.moderate_vegetation_fraction, 1)}</span>
                <span>Bare {fmtPct(sceneStats.bare_fraction, 1)}</span>
                <span>Clear {fmtPct(sceneStats.clear_fraction, 1)}</span>
              </>
            )}
            {stats && (
              <>
                <span>New water {fmtPct(stats.new_water_fraction, 1)}</span>
                <span>Veg loss {fmtPct(stats.vegetation_loss_fraction, 1)}</span>
                <span>Burn {fmtPct(stats.burn_fraction, 1)}</span>
                <span>Compared {fmtPct(stats.compared_fraction, 1)}</span>
              </>
            )}
            <span className="muted">{change?.attribution ?? image?.attribution}</span>
          </div>
        )}
      </div>
    </div>
  );
}
