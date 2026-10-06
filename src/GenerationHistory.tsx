import { translatePhase } from "./localization";
import { currentLanguage, message, t } from "./i18n";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { assistantUsageLabel, type AssistantUsage } from "./assistantUsage";

type Summary = { id: string; createdAt: number; request: string; status: string; source: string };
type Record = Summary & { provider?: string; model?: string; usage?: AssistantUsage; transcript?: string; output?: string; rawOutput?: string; systemInstructions?: string; userPrompt?: string; screenshot?: string; message?: string; events?: { phase: string; at: number; message: string }[] };

export default function GenerationHistory() {
  const [entries, setEntries] = useState<Summary[]>([]);
  const [record, setRecord] = useState<Record | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  async function refresh() {
    setLoading(true); setError("");
    try { setEntries(await invoke<Summary[]>("generation_history")); }
    catch (cause) { setError(String(cause)); }
    finally { setLoading(false); }
  }
  useEffect(() => {
    void refresh();
    const off = listen<string>("texthalo:generation-history-error", ({ payload }) => setError(payload));
    return () => { void off.then(fn => fn()); };
  }, []);
  async function open(id: string) {
    setError("");
    try { setRecord(await invoke<Record>("generation_history", { id })); }
    catch (cause) { setError(String(cause)); }
  }
  const textStyle = { whiteSpace: "pre-wrap" as const, overflowWrap: "anywhere" as const, maxHeight: 320, overflow: "auto" };
  return <div>
    <h1 className="pane-title">{t("Assistant history")}</h1>
    <p className="card-note">{t("Latest 50 runs, stored on this Mac, including requests, prompts, answers, and captured screenshots. Cloud requests also send text context to the hosted model. Older local runs and their screenshots are removed automatically.")}</p>
    <button className="plain" disabled={loading} onClick={() => void refresh()}>{t("Refresh")}</button>
    {error && <p role="alert">{message(error)}</p>}
    {loading && <p>{t("Loading runs…")}</p>}
    {!loading && entries.length === 0 && <p>{t("No Assistant runs saved yet.")}</p>}
    {record && <article className="permission-row" aria-label={t("Generation record")}>
      <div className="inline"><h2>{t("Run details")}</h2><button className="plain" onClick={() => setRecord(null)}>{t("Close details")}</button></div>
      <p>{new Date(record.createdAt).toLocaleString(currentLanguage())} · {record.source} · {translatePhase(record.status, currentLanguage())}</p>
      {record.model && <p className="card-note">{record.model}</p>}
      {record.usage && <p className="card-note">{assistantUsageLabel(record.usage)}{record.usage.refundedCredits ? t(" · {0} reserved credits returned", [record.usage.refundedCredits.toLocaleString(currentLanguage())]) : ""}</p>}
      {record.message && <p>{message(record.message)}</p>}
      {[ [t("Request"), record.request], [t("Speech transcript"), record.transcript], [t("Generated answer"), record.output] ].map(([label, value]) => value ? <section key={label}><h3>{label}</h3><p style={textStyle}>{value}</p></section> : null)}
      {record.screenshot && <details><summary>{t("Captured screenshot")}</summary><img src={record.screenshot} alt={t("Screenshot captured for this Assistant run")} style={{ maxWidth: "100%", height: "auto" }} /></details>}
      {[ [t("System instructions"), record.systemInstructions], [t("Model input"), record.userPrompt], [t("Raw model output (including delivery marker)"), record.rawOutput] ].map(([label, value]) => value ? <details key={label}><summary>{label}</summary><pre style={textStyle}>{value}</pre></details> : null)}
      <details><summary>{t("Run timeline")}</summary><ol>{record.events?.map((event, index) => <li key={index}>{new Date(event.at).toLocaleTimeString(currentLanguage())} · {translatePhase(event.phase, currentLanguage())} · {message(event.message)}</li>)}</ol></details>
    </article>}
    {entries.map(entry => <article className="permission-row" key={entry.id}>
      <div className="card-note">{new Date(entry.createdAt).toLocaleString(currentLanguage())} · {entry.source} · {translatePhase(entry.status, currentLanguage())}</div>
      <p style={textStyle}>{entry.request || t("No request transcribed")}</p>
      <button className="plain accent-action" onClick={() => void open(entry.id)}>{t("View run")}</button>
    </article>)}
  </div>;
}
