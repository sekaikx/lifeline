import { useEffect, useRef } from "react";
import { NumberField, SelectField } from "../components/Field";
import { fmt, fmtClock, fmtKg, fmtMoney, fmtPct } from "../lib/format";
import type { Palette } from "../lib/theme";
import { INK_ON_MODE, modeColor } from "./MapView";
import type { Item } from "../supply/types";
import type { Assignment, Mode, ModeChoice, ModeOption, Plan, PlanScenario, Priority, Request, RequestLine } from "./types";

/** Demand and transit limit that follow from a request's catalogue lines. */
export function linesPatch(lines: RequestLine[], catalogue: Item[]): Partial<Request> {
  if (!lines.length) return { items: [], max_transit_h: null };
  let kg = 0;
  let limit: number | null = null;
  for (const l of lines) {
    const it = catalogue.find((c) => c.id === l.item);
    if (!it) continue;
    kg += it.unit_kg * l.quantity;
    if (it.max_transit_h != null) limit = limit == null ? it.max_transit_h : Math.min(limit, it.max_transit_h);
  }
  return { items: lines, demand_kg: Math.max(Number(kg.toFixed(3)), 0.1), max_transit_h: limit };
}

const MODE_LABEL: Record<Mode, string> = { truck: "Truck", drone: "Drone", airdrop: "Aircraft" };
const PRIORITY_LABEL: Record<Priority, string> = { critical: "Critical", high: "High", routine: "Routine" };

interface Props {
  scenario: PlanScenario;
  plan: Plan | null;
  palette: Palette;
  selected: string | null;
  onSelect: (id: string | null) => void;
  onChangeRequest: (id: string, patch: Partial<Request>) => void;
  onDeleteRequest: (id: string) => void;
  onAddRequest: () => void;
  adding: boolean;
  /** Supply catalogue and units available at the depot, by item */
  catalogue: Item[];
  available: Record<string, number>;
  /** Shortfalls by request, from the supply allocation */
  shortfalls: Record<string, string[]>;
}

function Badge({ id, mode, palette, late }: { id: string; mode: Mode | null; palette: Palette; late?: boolean }) {
  return (
    <span
      className="badge"
      style={{
        background: mode ? modeColor(palette, mode) : "transparent",
        color: mode ? INK_ON_MODE : palette.critical,
        boxShadow: mode && !late ? undefined : `inset 0 0 0 1.5px ${palette.critical}`,
      }}
    >
      {id}
    </span>
  );
}

export function PlanPanel(props: Props) {
  const { scenario, plan, palette, selected } = props;
  const byId = new Map(plan?.assignments.map((a) => [a.request_id, a]) ?? []);
  const request = scenario.requests.find((r) => r.id === selected) ?? null;
  const assignment = request ? byId.get(request.id) : undefined;

  return (
    <aside className="plan-panel">
      {plan ? <Summary plan={plan} scenario={scenario} /> : <div className="panel-block muted">Planning…</div>}

      <section className="panel-block">
        <div className="block-head">
          <h2>Requests</h2>
          <button className="link-btn" onClick={props.onAddRequest} aria-pressed={props.adding}>
            {props.adding ? "Click the map…" : "Add request"}
          </button>
        </div>
        {scenario.requests.length === 0 && <p className="note">No requests yet. Choose Add request, then click the map where supplies are needed.</p>}
        {(["critical", "high", "routine"] as Priority[]).map((p) => {
          const rows = scenario.requests.filter((r) => r.priority === p);
          if (!rows.length) return null;
          return (
            <div key={p} className="req-group">
              <div className="req-group-label">{PRIORITY_LABEL[p]}</div>
              {rows.map((r) => {
                const a = byId.get(r.id);
                const late = !!a?.mode && !a.on_time;
                return (
                  <button key={r.id} className="req-row" aria-pressed={r.id === selected} onClick={() => props.onSelect(r.id === selected ? null : r.id)}>
                    <Badge id={r.id} mode={a?.mode ?? null} palette={palette} late={late} />
                    <span className="req-name">{r.name}</span>
                    <span className="req-meta num">
                      {a?.mode ? (
                        <>
                          <span className={late ? "late" : undefined}>{fmtClock(a.completion_h)}</span>
                          <span className="muted"> / {fmtClock(r.due_h)}</span>
                        </>
                      ) : (
                        <span className="late">{a ? "unserved" : ""}</span>
                      )}
                    </span>
                  </button>
                );
              })}
            </div>
          );
        })}
      </section>

      {request && (
        <RequestDetail
          request={request}
          assignment={assignment}
          scenario={scenario}
          palette={palette}
          catalogue={props.catalogue}
          available={props.available}
          shortfalls={props.shortfalls[request.id] ?? []}
          onChange={(patch) => props.onChangeRequest(request.id, patch)}
          onDelete={() => props.onDeleteRequest(request.id)}
        />
      )}
    </aside>
  );
}

function Summary({ plan, scenario }: { plan: Plan; scenario: PlanScenario }) {
  const s = plan.summary;
  const late = plan.assignments.filter((a) => a.mode && !a.on_time);
  const unserved = plan.assignments.filter((a) => !a.mode);
  const risky = plan.assignments.filter((a) => a.mode && !a.within_tolerance);
  const name = (id: string) => scenario.requests.find((r) => r.id === id)?.name ?? id;
  return (
    <section className="panel-block">
      <h2>Plan</h2>
      <p className="headline num">
        {fmtKg(s.expected_kg)} of {fmtKg(s.demand_kg)} expected to arrive
      </p>
      <dl className="stats num">
        <dt>Critical on time</dt>
        <dd>
          {s.critical_on_time} of {s.critical_total}
        </dd>
        <dt>Last arrival</dt>
        <dd>{fmtClock(s.makespan_h)}</dd>
        <dt>Operating cost</dt>
        <dd>{fmtMoney(s.cost)}</dd>
        <dt>Risk tolerance</dt>
        <dd>{fmtPct(scenario.settings.min_delivery_prob)} arrival</dd>
      </dl>
      {(late.length > 0 || unserved.length > 0 || risky.length > 0) && (
        <ul className="issues">
          {unserved.map((a) => (
            <li key={`u${a.request_id}`}>
              <strong>{name(a.request_id)}</strong> cannot be served. {a.reason}
            </li>
          ))}
          {late.map((a) => (
            <li key={`l${a.request_id}`}>
              <strong>{name(a.request_id)}</strong> arrives late, at {fmtClock(a.completion_h)}.
            </li>
          ))}
          {risky.map((a) => (
            <li key={`r${a.request_id}`}>
              <strong>{name(a.request_id)}</strong>: no option meets the tolerance; best is {fmtPct(a.delivery_prob)}.
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function RequestDetail({
  request: r,
  assignment: a,
  scenario,
  palette,
  catalogue,
  available,
  shortfalls,
  onChange,
  onDelete,
}: {
  request: Request;
  assignment: Assignment | undefined;
  scenario: PlanScenario;
  palette: Palette;
  catalogue: Item[];
  available: Record<string, number>;
  shortfalls: string[];
  onChange: (patch: Partial<Request>) => void;
  onDelete: () => void;
}) {
  const options = a?.options ?? [];
  // bring the detail into view when a request is selected, not on every replan
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    ref.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [r.id]);
  return (
    <section className="panel-block detail" ref={ref} key={r.id}>
      <div className="block-head">
        <h2>
          {r.id} · {r.name}
        </h2>
        <button className="link-btn danger" onClick={onDelete}>
          Remove
        </button>
      </div>

      <div className="form-grid">
        <label className="field">
          <span>Name</span>
          <input className="text-input" value={r.name} onChange={(e) => onChange({ name: e.target.value })} />
        </label>
        {r.items?.length ? (
          <div className="field">
            <span>Demand</span>
            <span className="num computed">{fmtKg(r.demand_kg)}</span>
          </div>
        ) : (
          <NumberField label="Demand" unit="kg" min={0.1} step={50} value={r.demand_kg} onChange={(v) => onChange({ demand_kg: v })} />
        )}
        <NumberField label="Needed within" unit="h" min={0.1} step={0.5} value={r.due_h} onChange={(v) => onChange({ due_h: v })} />
        <SelectField<Priority>
          label="Priority"
          value={r.priority}
          options={[
            { value: "critical", label: "Critical" },
            { value: "high", label: "High" },
            { value: "routine", label: "Routine" },
          ]}
          onChange={(v) => onChange({ priority: v })}
        />
        <SelectField<ModeChoice>
          label="Mode"
          value={r.mode}
          options={[
            { value: "auto", label: "Planner decides" },
            { value: "truck", label: "Truck only" },
            { value: "drone", label: "Drone only" },
            { value: "airdrop", label: "Aircraft only" },
          ]}
          onChange={(v) => onChange({ mode: v })}
        />
      </div>

      <RequestItems request={r} catalogue={catalogue} available={available} shortfalls={shortfalls} onChange={onChange} />

      {a && (
        <>
          <h3>Options</h3>
          <table className="compare num">
            <thead>
              <tr>
                <th />
                {options.map((o) => (
                  <th key={o.mode} className={o.mode === a.mode ? "chosen" : undefined}>
                    <span className="dot" style={{ background: modeColor(palette, o.mode) }} />
                    {MODE_LABEL[o.mode]}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              <Row label="Travel time" options={options} chosen={a.mode} render={(o) => fmtClock(o.one_way_h)} />
              <Row label="Trips" options={options} chosen={a.mode} render={(o) => String(o.trips)} />
              <Row label="Chance of arrival" options={options} chosen={a.mode} render={(o) => fmtPct(o.delivery_prob, 1)} />
              <Row label="Cost" options={options} chosen={a.mode} render={(o) => fmtMoney(o.cost_per_trip * o.trips)} />
              <Row label="Distance" options={options} chosen={a.mode} render={(o) => `${fmt(o.distance_km, 1)} km`} />
            </tbody>
          </table>
          {options
            .filter((o) => !o.feasible && o.reason && o.reason !== "excluded for this request")
            .map((o) => (
              <p key={o.mode} className="note">
                {MODE_LABEL[o.mode]}: {o.reason}.
              </p>
            ))}
          <p className="note">
            Travel time is one trip from the depot, including loading. In the plan this request waits for vehicles
            behind higher priorities and finishes at {a.mode ? fmtClock(a.completion_h) : "—"}.
          </p>
          {options.map((o) => (o.feasible ? <ModeDetail key={o.mode} option={o} chosen={o.mode === a.mode} scenario={scenario} /> : null))}
        </>
      )}
    </section>
  );
}

function Row({
  label,
  options,
  chosen,
  render,
}: {
  label: string;
  options: ModeOption[];
  chosen: Mode | null;
  render: (o: ModeOption) => string;
}) {
  return (
    <tr>
      <th>{label}</th>
      {options.map((o) => (
        <td key={o.mode} className={o.mode === chosen ? "chosen" : undefined}>
          {o.feasible ? render(o) : "—"}
        </td>
      ))}
    </tr>
  );
}

function ModeDetail({ option: o, chosen, scenario }: { option: ModeOption; chosen: boolean; scenario: PlanScenario }) {
  const d = o.detail;
  return (
    <div className={`mode-detail${chosen ? " chosen" : ""}`}>
      <h4>{MODE_LABEL[o.mode]}</h4>
      {d.type === "truck" && (
        <>
          <p className="note">
            {fmt(d.offroad_km, 1)} km off-road. Route options from fastest to safest; the planner takes the fastest one
            that meets the {fmtPct(scenario.settings.min_delivery_prob)} tolerance.
          </p>
          <table className="mini num">
            <tbody>
              {d.alternatives.map((alt, i) => (
                <tr key={i} className={Math.abs(alt.survival - d.survival) < 1e-9 ? "chosen" : undefined}>
                  <td>{fmtClock(alt.time_h)} driving</td>
                  <td>{fmt(alt.distance_km, 1)} km</td>
                  <td>{fmtPct(alt.survival, 1)} safe</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      {d.type === "drone" && (
        <p className="note num">
          Round trip uses {fmt(d.energy_wh, 0)} of {fmt(d.usable_wh, 0)} Wh usable battery. Radio line of sight to the
          depot over {fmtPct(d.comms_fraction)} of the route; {fmtPct(d.survival_round_trip, 1)} chance the drone also
          returns.
        </p>
      )}
      {d.type === "airdrop" && (
        <p className="note num">
          {d.bundles_per_sortie} bundles per sortie. Released {fmt(d.time_of_fall_s, 0)} s above the drop zone; Monte
          Carlo gives {fmtPct(d.dz_probability, 1)} of bundles inside {fmt(scenario.fleet.aircraft.dz_radius_m, 0)} m
          (CEP {fmt(d.cep_m, 0)} m) and {fmtPct(d.survival, 1)} for the aircraft reaching the release point.
        </p>
      )}
    </div>
  );
}

/** What a request asks for, by catalogue item. Demand and the transit limit follow. */
function RequestItems({
  request: r,
  catalogue,
  available,
  shortfalls,
  onChange,
}: {
  request: Request;
  catalogue: Item[];
  available: Record<string, number>;
  shortfalls: string[];
  onChange: (patch: Partial<Request>) => void;
}) {
  const lines = r.items ?? [];
  const set = (next: RequestLine[]) => onChange(linesPatch(next, catalogue));
  const unused = catalogue.filter((c) => !lines.some((l) => l.item === c.id));
  if (!catalogue.length && !lines.length) {
    return <p className="note">Load stock in the Supply workspace to request specific items; demand is then their weight.</p>;
  }
  return (
    <div className="req-items">
      <div className="block-head">
        <h3>Items</h3>
        {unused.length > 0 && (
          <select
            className="select add-item"
            value=""
            aria-label="Add an item"
            onChange={(e) => e.target.value && set([...lines, { item: e.target.value, quantity: 1 }])}
          >
            <option value="">Add item…</option>
            {unused.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        )}
      </div>
      {lines.map((l, k) => {
        const it = catalogue.find((c) => c.id === l.item);
        const have = available[l.item] ?? 0;
        return (
          <div key={l.item} className="req-item">
            <div className="req-item-name">
              <span>{it?.name ?? l.item}</span>
              <span className="note num">
                {it ? `${fmtKg(it.unit_kg * l.quantity)} · ${have} ${it.unit} in stock` : "not in the catalogue"}
                {it?.cold_chain && ` · cold chain ${it.cold_chain.min_c}–${it.cold_chain.max_c} °C`}
                {it?.controlled && " · controlled"}
              </span>
            </div>
            <NumberInputCompact
              value={l.quantity}
              unit={it?.unit ?? ""}
              warn={have < l.quantity}
              onChange={(q) => set(lines.map((x, j) => (j === k ? { ...x, quantity: q } : x)))}
            />
            <button className="row-btn" aria-label={`Remove ${it?.name ?? l.item}`} onClick={() => set(lines.filter((_, j) => j !== k))}>
              ✕
            </button>
          </div>
        );
      })}
      {r.max_transit_h != null && lines.length > 0 && (
        <p className="note">Cargo may spend at most {fmtClock(r.max_transit_h)} in transit; slower modes are not used.</p>
      )}
      {shortfalls.map((t) => (
        <p key={t} className="note late">
          {t}
        </p>
      ))}
    </div>
  );
}

function NumberInputCompact({ value, unit, warn, onChange }: { value: number; unit: string; warn: boolean; onChange: (v: number) => void }) {
  return (
    <div className={`input-wrap qty${warn ? " invalid" : ""}`} title={warn ? "More than is in stock" : undefined}>
      <input
        inputMode="decimal"
        aria-label="Quantity"
        defaultValue={value}
        key={value}
        onBlur={(e) => {
          const v = Number(e.target.value);
          if (Number.isFinite(v) && v > 0 && v !== value) onChange(v);
          else e.target.value = String(value);
        }}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
      <span className="unit">{unit}</span>
    </div>
  );
}
