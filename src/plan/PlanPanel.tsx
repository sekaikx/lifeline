import { NumberField, SelectField } from "../components/Field";
import { fmt, fmtClock, fmtKg, fmtMoney, fmtPct } from "../lib/format";
import type { Palette } from "../lib/theme";
import { INK_ON_MODE, modeColor } from "./MapView";
import type { Assignment, Mode, ModeChoice, ModeOption, Plan, PlanScenario, Priority, Request } from "./types";

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
  onChange,
  onDelete,
}: {
  request: Request;
  assignment: Assignment | undefined;
  scenario: PlanScenario;
  palette: Palette;
  onChange: (patch: Partial<Request>) => void;
  onDelete: () => void;
}) {
  const options = a?.options ?? [];
  return (
    <section className="panel-block detail" ref={(el) => el?.scrollIntoView({ block: "nearest", behavior: "smooth" })} key={r.id}>
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
        <NumberField label="Demand" unit="kg" min={0.1} step={50} value={r.demand_kg} onChange={(v) => onChange({ demand_kg: v })} />
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
