import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { Section } from "../components/Field";
import { api, errorText } from "../lib/api";
import type { PlanState, WorkspaceHandle } from "../plan/PlanWorkspace";
import type { Item } from "../supply/types";
import { AiSettingsDialog } from "./AiSettingsDialog";
import type { AiSettings, AreaAssessment, Message, ParsedRequest } from "./types";

export function AssistantWorkspace({
  planState,
  planHandle,
  catalogue,
  scenePng,
  sceneBounds,
  onStatus,
}: {
  planState: PlanState | null;
  planHandle: MutableRefObject<WorkspaceHandle | null>;
  catalogue: Item[];
  scenePng: string | null;
  sceneBounds: [number, number, number, number] | null;
  onStatus: (s: string) => void;
}) {
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [parsed, setParsed] = useState<ParsedRequest | null>(null);
  const [assessment, setAssessment] = useState<AreaAssessment | null>(null);
  const bottom = useRef<HTMLDivElement>(null);

  const reloadSettings = () => {
    api
      .aiSettings()
      .then(setSettings)
      .catch((e) => setError(errorText(e)));
  };

  useEffect(() => {
    reloadSettings();
  }, []);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [messages, busy]);

  const context = () => {
    const s = planState?.scenario;
    if (!s) return "No plan loaded.";
    const lines = [
      `Plan: ${s.name}`,
      `Depot: ${s.depot.name} at (${s.depot.x.toFixed(0)}, ${s.depot.y.toFixed(0)})`,
      `Requests: ${s.requests.length}`,
      ...s.requests.slice(0, 12).map((r) => `${r.id} ${r.name} ${r.demand_kg}kg ${r.priority} due ${r.due_h}h`),
    ];
    if (planState?.plan) {
      const sum = planState.plan.summary;
      lines.push(
        `Expected ${sum.expected_kg.toFixed(0)} of ${sum.demand_kg.toFixed(0)} kg, cost ${sum.cost.toFixed(0)}, unserved ${sum.unserved}`,
      );
    }
    return lines.join("\n");
  };

  const send = async () => {
    const text = input.trim();
    if (!text || !settings?.enabled) return;
    const next: Message[] = [...messages, { role: "user", content: [{ type: "text", text }] }];
    setMessages(next);
    setInput("");
    setBusy("Thinking…");
    setError(null);
    try {
      const reply = await api.aiChat(context(), next);
      setMessages((m) => [...m, { role: "assistant", content: [{ type: "text", text: reply.text }] }]);
      onStatus(`${reply.model} · ${reply.ms} ms${reply.local ? " · local" : ""}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const parseRadio = async () => {
    const text = input.trim();
    if (!text) return;
    setBusy("Parsing request…");
    setError(null);
    setParsed(null);
    try {
      const places = (planState?.world.places ?? []).slice(0, 40).map((p) => ({ name: p.name, x: p.x, y: p.y }));
      const cat = catalogue.map((i) => ({ id: i.id, name: i.name, unit: i.unit, unit_kg: i.unit_kg }));
      const r = await api.aiParseRequest(text, places, cat);
      setParsed(r);
      onStatus(`Parsed with ${r.model}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const addParsed = () => {
    if (!parsed || !planHandle.current) return;
    const id = planHandle.current.addRequest({
      name: parsed.name,
      x: parsed.x ?? planState?.scenario.depot.x ?? 0,
      y: parsed.y ?? planState?.scenario.depot.y ?? 0,
      demand_kg: parsed.demand_kg,
      priority: parsed.priority,
      due_h: parsed.due_h,
      mode: parsed.mode,
    });
    onStatus(`Added ${id} to the plan`);
    setParsed(null);
  };

  const assess = async () => {
    if (!scenePng || !sceneBounds) {
      setError("Open Area intel and render a scene first.");
      return;
    }
    setBusy("Reading image…");
    setError(null);
    setAssessment(null);
    try {
      const a = await api.aiAssessArea(scenePng, sceneBounds, context());
      setAssessment(a);
      onStatus(`${a.model} assessed the scene`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="split-workspace">
      <aside className="setup">
        <Section title="Assistant">
          <p className="note">
            Off by default. Uses Lifeline&apos;s own numbers; turn on a local or free cloud model in settings.
          </p>
          <p className="note">
            {settings == null
              ? "Loading settings…"
              : settings.enabled
                ? settings.local_only
                  ? "On · local models only"
                  : "On"
                : "Off"}
            {settings?.chat ? ` · chat ${settings.chat.model}` : ""}
          </p>
          <button className="btn" onClick={() => setSettingsOpen(true)}>
            Settings
          </button>
        </Section>
        <Section title="Shortcuts">
          <button className="btn" disabled={!!busy || !input.trim()} onClick={() => void parseRadio()}>
            Parse as request
          </button>
          <button className="btn" disabled={!!busy || !scenePng} onClick={() => void assess()}>
            Read satellite image
          </button>
          <p className="note">Parse turns radio-style text into a request. Image read needs a scene from Area intel.</p>
        </Section>
        {parsed && (
          <Section title="Parsed request">
            <p>
              <strong>{parsed.name}</strong> · {parsed.demand_kg} kg · {parsed.priority} · due {parsed.due_h} h
            </p>
            {parsed.place && <p className="note">Place: {parsed.place}</p>}
            {parsed.notes && <p className="note">{parsed.notes}</p>}
            <button className="btn primary" onClick={addParsed}>
              Add to plan
            </button>
          </Section>
        )}
        {assessment && (
          <Section title="Image reading">
            <p className="note">{assessment.summary}</p>
            <ul className="issues">
              {assessment.findings.slice(0, 8).map((f, i) => (
                <li key={i}>
                  <strong>{f.cell}</strong> {f.kind}: {f.note} ({f.confidence})
                </li>
              ))}
            </ul>
          </Section>
        )}
        {busy && <p className="note">{busy}</p>}
        {error && <p className="form-error">{error}</p>}
      </aside>
      <div className="assistant-main">
        <div className="chat-log">
          {messages.length === 0 && (
            <div className="empty">
              <div>
                <h1 className="empty-title">Assistant</h1>
                <p>
                  {settings?.enabled
                    ? "Ask about the current plan, or paste a radio message and parse it as a request."
                    : "Open Settings and turn the assistant on. Prefer a local model if you can."}
                </p>
              </div>
            </div>
          )}
          {messages.map((m, i) => (
            <div key={i} className={`chat-bubble ${m.role}`}>
              {m.content.map((p, j) => (p.type === "text" ? <p key={j}>{p.text}</p> : null))}
            </div>
          ))}
          <div ref={bottom} />
        </div>
        <form
          className="chat-compose"
          onSubmit={(e) => {
            e.preventDefault();
            void send();
          }}
        >
          <textarea
            className="text-input chat-input"
            rows={2}
            value={input}
            placeholder={settings?.enabled ? "Message…" : "Turn the assistant on in Settings"}
            disabled={!settings?.enabled || !!busy}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
          />
          <button className="btn primary" type="submit" disabled={!settings?.enabled || !!busy || !input.trim()}>
            Send
          </button>
        </form>
      </div>
      <AiSettingsDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        onSaved={(s) => {
          setSettings(s);
          setSettingsOpen(false);
          onStatus("Assistant settings saved");
        }}
      />
    </div>
  );
}
