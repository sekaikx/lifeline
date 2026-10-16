import { Fragment, useEffect, useId, useState } from "react";
import { api, errorText } from "../lib/api";
import { Badge, Dialog } from "../components/Dialog";
import { Section } from "../components/Field";
import type { AiSettings, ModelInfo, ModelRef, Preset, PublicProvider, Role } from "./types";

const ROLES: { role: Role; label: string; help: string; hint?: string }[] = [
  { role: "chat", label: "Chat", help: "Analyst conversation and reports" },
  {
    role: "vision",
    label: "Vision",
    help: "Reading satellite images",
    hint: "Must accept images, e.g. Gemma 3, Qwen2.5-VL, Gemini Flash, Llama 4 Scout.",
  },
  {
    role: "fast",
    label: "Fast",
    help: "Small jobs: turning a radio message into a request",
    hint: "Leave empty to use the chat model.",
  },
];

const CUSTOM = "__custom__";

/** Payload for the backend: a key is sent only when the user typed or cleared it, otherwise null. */
function toPayload(s: AiSettings): AiSettings {
  return { ...s, providers: s.providers.map((p) => ({ ...p, new_key: p.new_key ?? null })) };
}

/** Settings as the backend returned them: no pending key changes. */
function fromServer(s: AiSettings): AiSettings {
  return { ...s, providers: s.providers.map((p) => ({ ...p, new_key: null })) };
}

const snapshot = (s: AiSettings): string => JSON.stringify(toPayload(s));

const modelLabel = (m: ModelInfo): string =>
  [m.id, m.free ? "free" : null, m.vision ? "images" : null].filter(Boolean).join(" · ");

const presetLabel = (p: Preset): string =>
  `${p.name}${p.free ? " · free" : ""}${p.local ? " · on this computer" : ""}`;

function without<T>(rec: Record<string, T>, key: string): Record<string, T> {
  return Object.fromEntries(Object.entries(rec).filter(([k]) => k !== key));
}

export function AiSettingsDialog({
  open,
  onClose,
  onSaved,
}: {
  open: boolean;
  onClose: () => void;
  onSaved: (s: AiSettings) => void;
}) {
  const baseId = useId();
  const [draft, setDraft] = useState<AiSettings | null>(null);
  const [presets, setPresets] = useState<Preset[]>([]);
  /** Snapshot of the last saved (or loaded) settings, to tell whether the draft has unsaved changes. */
  const [saved, setSaved] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [models, setModels] = useState<Record<string, ModelInfo[]>>({});
  const [modelError, setModelError] = useState<Record<string, string>>({});
  const [fetching, setFetching] = useState<string | null>(null);
  const [pick, setPick] = useState("");

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setDraft(null);
    setLoadError(null);
    setSaveError(null);
    setModels({});
    setModelError({});
    setPick("");
    Promise.all([api.aiSettings(), api.aiPresets()])
      .then(([settings, list]) => {
        if (cancelled) return;
        const loaded = fromServer(settings);
        setDraft(loaded);
        setSaved(snapshot(loaded));
        setPresets(list);
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(errorText(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const update = (fn: (d: AiSettings) => AiSettings) => setDraft((d) => (d ? fn(d) : d));

  const updateProvider = (id: string, patch: Partial<PublicProvider>) =>
    update((d) => ({ ...d, providers: d.providers.map((p) => (p.id === id ? { ...p, ...patch } : p)) }));

  const setRole = (role: Role, ref: ModelRef | null) => update((d) => ({ ...d, [role]: ref }));

  /** Saves the draft and keeps the dialog open. Resolves to true on success. */
  const persist = async (): Promise<boolean> => {
    if (!draft) return false;
    setSaving(true);
    setSaveError(null);
    try {
      const result = fromServer(await api.aiSaveSettings(toPayload(draft)));
      setDraft(result);
      setSaved(snapshot(result));
      onSaved(result);
      return true;
    } catch (e) {
      setSaveError(errorText(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const save = async () => {
    if (await persist()) onClose();
  };

  const fetchModels = async (id: string) => {
    if (!draft) return;
    // The backend only lists models for saved providers, so save first when anything is unsaved.
    if (snapshot(draft) !== saved && !(await persist())) return;
    setFetching(id);
    setModelError((e) => without(e, id));
    try {
      const list = await api.aiModels(id);
      setModels((m) => ({ ...m, [id]: list }));
    } catch (e) {
      setModelError((m) => ({ ...m, [id]: errorText(e) }));
    } finally {
      setFetching(null);
    }
  };

  const removeProvider = (id: string) => {
    update((d) => ({
      ...d,
      providers: d.providers.filter((p) => p.id !== id),
      chat: d.chat?.provider === id ? null : d.chat,
      vision: d.vision?.provider === id ? null : d.vision,
      fast: d.fast?.provider === id ? null : d.fast,
    }));
    setModels((m) => without(m, id));
    setModelError((m) => without(m, id));
  };

  const addProvider = () => {
    update((d) => {
      if (pick === CUSTOM) {
        let n = 1;
        while (d.providers.some((p) => p.id === `custom-${n}`)) n++;
        const custom: PublicProvider = {
          id: `custom-${n}`,
          name: "Custom server",
          kind: "open_ai",
          base_url: "http://localhost:8080/v1",
          local: true,
          has_key: false,
          new_key: null,
        };
        return { ...d, providers: [...d.providers, custom] };
      }
      const preset = presets.find((p) => p.id === pick);
      if (!preset) return d;
      const added: PublicProvider = {
        id: preset.id,
        name: preset.name,
        kind: preset.kind,
        base_url: preset.base_url,
        local: preset.local,
        has_key: false,
        new_key: null,
      };
      const next: AiSettings = { ...d, providers: [...d.providers, added] };
      for (const { role } of ROLES) {
        const model = preset[role];
        if (model && next[role] === null) next[role] = { provider: preset.id, model };
      }
      return next;
    });
    setPick("");
  };

  const pickedPreset = presets.find((p) => p.id === pick);

  return (
    <Dialog
      open={open}
      title="Assistant settings"
      onClose={onClose}
      wide
      footer={
        <>
          <button className="btn quiet" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={save} disabled={!draft || saving}>
            {saving ? "Saving…" : "Save"}
          </button>
        </>
      }
    >
      <p className="note">
        The assistant reads Lifeline's own numbers and satellite images and answers in plain language. Use a model on
        this computer (nothing leaves it) or a free cloud model. It is off until you turn it on.
      </p>

      {!draft &&
        (loadError ? <p className="form-error">{loadError}</p> : <p className="note">Loading…</p>)}

      {draft && (
        <>
          <label className="toggle">
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(e) => update((d) => ({ ...d, enabled: e.target.checked }))}
            />
            Assistant on
          </label>
          <label className="toggle">
            <input
              type="checkbox"
              checked={draft.local_only}
              onChange={(e) => update((d) => ({ ...d, local_only: e.target.checked }))}
            />
            Local models only (nothing leaves this computer or network)
          </label>

          <Section title="Providers">
            <div className="ai-providers">
              {draft.providers.length === 0 && <p className="note">No providers yet. Add one below.</p>}
              {draft.providers.map((p) => {
                const list = models[p.id];
                return (
                  <div key={p.id} className="ai-provider">
                    <div className="ai-provider-head">
                      <input
                        className="text-input"
                        aria-label="Provider name"
                        value={p.name}
                        onChange={(e) => updateProvider(p.id, { name: e.target.value })}
                      />
                      <Badge tone={p.local ? "good" : "neutral"}>{p.local ? "local" : "cloud"}</Badge>
                    </div>
                    <label className="field">
                      <span>Base URL</span>
                      <input
                        className="text-input"
                        value={p.base_url}
                        onChange={(e) => updateProvider(p.id, { base_url: e.target.value })}
                      />
                    </label>
                    <label className="field">
                      <span>API key</span>
                      <input
                        className="text-input"
                        type="password"
                        autoComplete="off"
                        value={p.new_key ?? ""}
                        placeholder={p.has_key ? "stored" : "none"}
                        onChange={(e) => updateProvider(p.id, { new_key: e.target.value })}
                      />
                    </label>
                    {p.new_key === "" && p.has_key && <div className="small-muted">Key will be removed on save.</div>}
                    {p.new_key && <div className="small-muted">New key, saved with Save or Fetch models.</div>}
                    <div className="ai-provider-head">
                      <button
                        className="btn"
                        onClick={() => fetchModels(p.id)}
                        disabled={saving || fetching !== null}
                      >
                        {fetching === p.id ? "Fetching…" : "Fetch models"}
                      </button>
                      <span>
                        {p.has_key && p.new_key !== "" && (
                          <button className="link-btn" onClick={() => updateProvider(p.id, { new_key: "" })}>
                            Clear key
                          </button>
                        )}{" "}
                        <button className="link-btn danger" onClick={() => removeProvider(p.id)}>
                          Remove
                        </button>
                      </span>
                    </div>
                    {list && <div className="small-muted">{list.length} models</div>}
                    {modelError[p.id] && <p className="form-error">{modelError[p.id]}</p>}
                  </div>
                );
              })}
            </div>

            <div className="ai-add">
              <select className="select" aria-label="Provider to add" value={pick} onChange={(e) => setPick(e.target.value)}>
                <option value="">Choose a provider to add…</option>
                {presets
                  .filter((p) => !draft.providers.some((q) => q.id === p.id))
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {presetLabel(p)}
                    </option>
                  ))}
                <option value={CUSTOM}>Custom server (OpenAI-compatible)</option>
              </select>
              <button className="btn" onClick={addProvider} disabled={!pick}>
                Add
              </button>
            </div>
            {pickedPreset && (
              <>
                <p className="note">{pickedPreset.note}</p>
                <div className="small-muted">{pickedPreset.url}</div>
              </>
            )}
            {pick === CUSTOM && (
              <p className="note">
                Any server that speaks the OpenAI chat API, such as llama.cpp or LM Studio on this computer. You can edit
                its name and base URL after adding it.
              </p>
            )}
          </Section>

          <Section title="Models for each job">
            <div className="ai-roles">
              {ROLES.map(({ role, label, help, hint }) => {
                const ref = draft[role];
                const selected = ref ? draft.providers.find((p) => p.id === ref.provider) : undefined;
                const datalistId = `${baseId}-${role}`;
                const list = ref ? (models[ref.provider] ?? []) : [];
                const blocked = draft.local_only && selected !== undefined && !selected.local;
                return (
                  <Fragment key={role}>
                    <div className="ai-role">
                      <div>
                        <strong>{label}</strong>
                        <div className="small-muted">{help}</div>
                      </div>
                      <select
                        className="select"
                        aria-label={`${label} provider`}
                        value={ref?.provider ?? ""}
                        onChange={(e) => setRole(role, e.target.value ? { provider: e.target.value, model: "" } : null)}
                      >
                        <option value="">— none —</option>
                        {draft.providers.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.name}
                          </option>
                        ))}
                      </select>
                      <input
                        className="text-input"
                        aria-label={`${label} model`}
                        list={datalistId}
                        disabled={!ref}
                        value={ref?.model ?? ""}
                        placeholder={role === "fast" ? "same as chat" : "model id"}
                        onChange={(e) => {
                          if (ref) setRole(role, { ...ref, model: e.target.value });
                        }}
                      />
                      <datalist id={datalistId}>
                        {list.map((m) => (
                          <option key={m.id} value={m.id} label={modelLabel(m)} />
                        ))}
                      </datalist>
                    </div>
                    {hint && <div className="small-muted">{hint}</div>}
                    {blocked && <p className="form-error">Cloud provider — blocked while 'local only' is on</p>}
                  </Fragment>
                );
              })}
            </div>
          </Section>

          {saveError && <p className="form-error">{saveError}</p>}
          {draft.path && (
            <div className="small-muted">
              Stored in {draft.path}. API keys stay in this file and are never shown again.
            </div>
          )}
        </>
      )}
    </Dialog>
  );
}
