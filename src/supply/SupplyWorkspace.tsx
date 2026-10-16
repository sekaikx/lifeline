import { useCallback, useEffect, useMemo, useState } from "react";
import { NumberField, Section, SelectField } from "../components/Field";
import { api, errorText } from "../lib/api";
import { fmt, fmtKg } from "../lib/format";
import type { Plan, PlanScenario } from "../plan/types";
import type { SupplyView } from "../plan/PlanWorkspace";
import {
  ACTIONS,
  CLASSES,
  classLabel,
  todayIso,
  type Action,
  type Allocation,
  type Snapshot,
  type TxDraft,
} from "./types";

function availableByItem(snap: Snapshot): Record<string, number> {
  const out: Record<string, number> = {};
  for (const line of snap.stock) out[line.item] = (out[line.item] ?? 0) + line.available;
  return out;
}

function shortfallsByRequest(alloc: Allocation | null): Record<string, string[]> {
  if (!alloc) return {};
  const out: Record<string, string[]> = {};
  for (const s of alloc.shortfalls) {
    (out[s.request_id] ??= []).push(`${s.item}: need ${s.needed}, have ${s.picked} (${s.reason})`);
  }
  return out;
}

export function SupplyWorkspace({
  scenario,
  plan,
  onSupply,
  onStatus,
}: {
  scenario: PlanScenario | null;
  plan: Plan | null;
  onSupply: (s: SupplyView) => void;
  onStatus: (s: string) => void;
}) {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [alloc, setAlloc] = useState<Allocation | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [user, setUser] = useState("planner");
  const [draft, setDraft] = useState<TxDraft>({
    action: "receive",
    item: "",
    lot: "",
    expiry: null,
    from: null,
    to: null,
    qty: 1,
    reference: null,
    user: "planner",
    witness: null,
    note: "",
  });

  const publish = useCallback(
    (s: Snapshot, a: Allocation | null) => {
      onSupply({
        catalogue: s.items,
        available: availableByItem(s),
        shortfalls: shortfallsByRequest(a),
      });
    },
    [onSupply],
  );

  const refresh = useCallback(async () => {
    setBusy("Loading stock…");
    setError(null);
    try {
      const s = await api.supplySnapshot(todayIso());
      setSnap(s);
      let a: Allocation | null = null;
      if (scenario && plan && s.locations.length) {
        a = await api.supplyAllocate(
          scenario,
          plan,
          s.locations.map((l) => l.id),
          todayIso(),
        );
        setAlloc(a);
      } else {
        setAlloc(null);
      }
      publish(s, a);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      onStatus("");
    }
  }, [scenario, plan, publish, onStatus]);

  // initial load only; plan/scenario changes re-allocate below
  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!snap || !scenario || !plan || !snap.locations.length) {
      setAlloc(null);
      if (snap) publish(snap, null);
      return;
    }
    let cancelled = false;
    api
      .supplyAllocate(
        scenario,
        plan,
        snap.locations.map((l) => l.id),
        todayIso(),
      )
      .then((a) => {
        if (cancelled) return;
        setAlloc(a);
        publish(snap, a);
      })
      .catch((e) => {
        if (!cancelled) setError(errorText(e));
      });
    return () => {
      cancelled = true;
    };
  }, [scenario, plan, snap, publish]);

  useEffect(() => {
    if (snap?.items.length && !draft.item) {
      setDraft((d) => ({ ...d, item: snap.items[0].id, to: snap.locations[0]?.id ?? null }));
    }
  }, [snap, draft.item]);

  const itemName = useMemo(() => {
    const m = new Map(snap?.items.map((i) => [i.id, i]) ?? []);
    return (id: string) => m.get(id)?.name ?? id;
  }, [snap]);

  const loadExample = async () => {
    setBusy("Loading example stock…");
    try {
      await api.supplyLoadExample(todayIso());
      await refresh();
      onStatus("Example supply loaded");
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  };

  const apply = async () => {
    setBusy("Posting movement…");
    setError(null);
    try {
      const tx = await api.supplyApply({ ...draft, user });
      onStatus(`Posted ${tx.action} #${tx.seq}`);
      await refresh();
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  };

  const doImport = async () => {
    try {
      const text = await api.openText("csv");
      if (!text) return;
      setBusy("Importing…");
      const loc = snap?.locations[0]?.id ?? "depot";
      const report = await api.supplyImport(text, user, loc);
      onStatus(`Imported ${report.received} of ${report.rows} rows`);
      if (report.errors.length) setError(report.errors.slice(0, 3).join("; "));
      await refresh();
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  };

  const doExport = async (kind: "stock" | "ledger") => {
    try {
      const csv = await api.supplyExport(kind);
      const path = await api.saveText(`lifeline-${kind}.csv`, "csv", csv);
      if (path) onStatus(`Saved ${path}`);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="split-workspace">
      <aside className="setup">
        <Section title="Stock">
          <p className="note">Lot-tracked supply with a tamper-evident ledger. Plan uses this catalogue for request lines.</p>
          <div className="btn-row">
            <button className="btn" disabled={!!busy} onClick={() => void refresh()}>
              Refresh
            </button>
            <button className="btn" disabled={!!busy} onClick={() => void loadExample()}>
              Load example
            </button>
          </div>
          <div className="btn-row">
            <button className="btn quiet" disabled={!!busy} onClick={() => void doImport()}>
              Import CSV
            </button>
            <button className="btn quiet" disabled={!!busy || !snap} onClick={() => void doExport("stock")}>
              Export stock
            </button>
            <button className="btn quiet" disabled={!!busy || !snap} onClick={() => void doExport("ledger")}>
              Export ledger
            </button>
          </div>
          {snap && (
            <p className="note num">
              {snap.stock.length} lots · ledger {snap.ledger_len}
              {snap.integrity ? ` · ${snap.integrity}` : ""}
            </p>
          )}
        </Section>
        <Section title="Movement">
          <label className="field">
            <span>User</span>
            <input className="text-input" value={user} onChange={(e) => setUser(e.target.value)} />
          </label>
          <SelectField
            label="Action"
            value={draft.action}
            options={ACTIONS.map((a) => ({ value: a.value, label: a.label }))}
            onChange={(v) => setDraft((d) => ({ ...d, action: v as Action }))}
          />
          <SelectField
            label="Item"
            value={draft.item}
            options={(snap?.items ?? []).map((i) => ({ value: i.id, label: `${i.id} · ${i.name}` }))}
            onChange={(v) => setDraft((d) => ({ ...d, item: v }))}
          />
          <label className="field">
            <span>Lot</span>
            <input className="text-input" value={draft.lot} onChange={(e) => setDraft((d) => ({ ...d, lot: e.target.value }))} />
          </label>
          <label className="field">
            <span>Expiry</span>
            <input
              className="text-input"
              type="date"
              value={draft.expiry ?? ""}
              onChange={(e) => setDraft((d) => ({ ...d, expiry: e.target.value || null }))}
            />
          </label>
          <NumberField label="Quantity" min={0.001} step={1} value={draft.qty} onChange={(v) => setDraft((d) => ({ ...d, qty: v }))} />
          <SelectField
            label="From"
            value={draft.from ?? ""}
            options={[{ value: "", label: "—" }, ...(snap?.locations ?? []).map((l) => ({ value: l.id, label: l.name }))]}
            onChange={(v) => setDraft((d) => ({ ...d, from: v || null }))}
          />
          <SelectField
            label="To"
            value={draft.to ?? ""}
            options={[{ value: "", label: "—" }, ...(snap?.locations ?? []).map((l) => ({ value: l.id, label: l.name }))]}
            onChange={(v) => setDraft((d) => ({ ...d, to: v || null }))}
          />
          <label className="field">
            <span>Witness</span>
            <input
              className="text-input"
              value={draft.witness ?? ""}
              onChange={(e) => setDraft((d) => ({ ...d, witness: e.target.value || null }))}
              placeholder="required for controlled items"
            />
          </label>
          <label className="field">
            <span>Note</span>
            <input className="text-input" value={draft.note} onChange={(e) => setDraft((d) => ({ ...d, note: e.target.value }))} />
          </label>
          <button className="btn primary" disabled={!!busy || !draft.item || !draft.lot} onClick={() => void apply()}>
            Post
          </button>
        </Section>
        {busy && <p className="note">{busy}</p>}
        {error && <p className="form-error">{error}</p>}
      </aside>
      <div className="supply-main">
        {!snap ? (
          <div className="empty">
            <div>
              <h1 className="empty-title">Supply</h1>
              <p>{busy ?? "Loading stock…"}</p>
            </div>
          </div>
        ) : (
          <>
            {snap.alerts.length > 0 && (
              <section className="panel-block">
                <h2>Alerts</h2>
                <ul className="issues">
                  {snap.alerts.map((a, i) => (
                    <li key={i}>
                      {a.kind === "low" && (
                        <>
                          <strong>{itemName(a.item)}</strong> low: {fmt(a.available, 0)} (reorder {fmt(a.reorder_level, 0)})
                        </>
                      )}
                      {a.kind === "expiring" && (
                        <>
                          <strong>{itemName(a.item)}</strong> lot {a.lot} expires {a.expiry} ({a.days}d, qty {fmt(a.quantity, 0)})
                        </>
                      )}
                      {a.kind === "expired" && (
                        <>
                          <strong>{itemName(a.item)}</strong> lot {a.lot} expired {a.expiry} (qty {fmt(a.quantity, 0)})
                        </>
                      )}
                    </li>
                  ))}
                </ul>
              </section>
            )}
            <section className="panel-block">
              <h2>On hand</h2>
              <table className="data-table">
                <thead>
                  <tr>
                    <th>Item</th>
                    <th>Class</th>
                    <th>Lot</th>
                    <th>Expiry</th>
                    <th>Location</th>
                    <th className="num">Avail</th>
                    <th className="num">Held</th>
                  </tr>
                </thead>
                <tbody>
                  {snap.stock.map((line) => {
                    const item = snap.items.find((i) => i.id === line.item);
                    return (
                      <tr key={`${line.item}-${line.lot}-${line.location}`}>
                        <td>{item?.name ?? line.item}</td>
                        <td>{item ? classLabel(item.class) : "—"}</td>
                        <td className="num">{line.lot}</td>
                        <td className="num">{line.expiry ?? "—"}</td>
                        <td>{snap.locations.find((l) => l.id === line.location)?.name ?? line.location}</td>
                        <td className="num">{fmt(line.available, 0)}</td>
                        <td className="num">{fmt(line.held, 0)}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {snap.stock.length === 0 && <p className="note">No lots yet. Load the example or import a CSV.</p>}
            </section>
            <section className="panel-block">
              <h2>Catalogue</h2>
              <table className="data-table">
                <thead>
                  <tr>
                    <th>Id</th>
                    <th>Name</th>
                    <th>Class</th>
                    <th>Unit</th>
                    <th className="num">kg</th>
                    <th>Flags</th>
                  </tr>
                </thead>
                <tbody>
                  {snap.items.map((i) => (
                    <tr key={i.id}>
                      <td className="num">{i.id}</td>
                      <td>{i.name}</td>
                      <td>{CLASSES.find((c) => c.value === i.class)?.label ?? i.class}</td>
                      <td>{i.unit}</td>
                      <td className="num">{fmtKg(i.unit_kg)}</td>
                      <td>
                        {[i.controlled ? "controlled" : null, i.cold_chain ? "cold" : null, i.hazmat].filter(Boolean).join(" · ") || "—"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
            {alloc && (
              <section className="panel-block">
                <h2>Allocation to plan</h2>
                <p className="note">
                  {alloc.picks.length} picks · {alloc.shortfalls.length} shortfalls · {alloc.manifests.length} trip manifests
                  {!scenario || !plan ? " · open a plan to allocate" : ""}
                </p>
                {alloc.shortfalls.length > 0 && (
                  <ul className="issues">
                    {alloc.shortfalls.slice(0, 12).map((s, i) => (
                      <li key={i}>
                        <strong>{s.request_id}</strong> {itemName(s.item)}: {s.reason}
                      </li>
                    ))}
                  </ul>
                )}
                {alloc.manifests.length > 0 && (
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th>Unit</th>
                        <th>Mode</th>
                        <th>Request</th>
                        <th className="num">kg</th>
                        <th>Flags</th>
                      </tr>
                    </thead>
                    <tbody>
                      {alloc.manifests.slice(0, 20).map((m, i) => (
                        <tr key={i}>
                          <td className="num">{m.unit}</td>
                          <td>{m.mode}</td>
                          <td>
                            {m.request_id} · {m.destination}
                          </td>
                          <td className="num">{fmtKg(m.kg)}</td>
                          <td>
                            {[m.cold_chain ? "cold" : null, m.controlled ? "controlled" : null, m.hazmat.length ? m.hazmat.join(",") : null]
                              .filter(Boolean)
                              .join(" · ") || "—"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>
            )}
            <section className="panel-block">
              <h2>Recent ledger</h2>
              <table className="data-table">
                <thead>
                  <tr>
                    <th className="num">#</th>
                    <th>Action</th>
                    <th>Item</th>
                    <th>Lot</th>
                    <th className="num">Qty</th>
                    <th>User</th>
                  </tr>
                </thead>
                <tbody>
                  {snap.recent.map((tx) => (
                    <tr key={tx.seq}>
                      <td className="num">{tx.seq}</td>
                      <td>{tx.action}</td>
                      <td>{itemName(tx.item)}</td>
                      <td className="num">{tx.lot}</td>
                      <td className="num">{fmt(tx.qty, 0)}</td>
                      <td>
                        {tx.user}
                        {tx.witness ? ` / ${tx.witness}` : ""}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          </>
        )}
      </div>
    </div>
  );
}
