import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Summary = { id: string; createdAt: number; request: string; status: string; source: string };
type Record = Summary & { transcript?: string; output?: string; rawOutput?: string; systemInstructions?: string; userPrompt?: string; screenshot?: string; message?: string; events?: { phase: string; at: number; message: string }[] };

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
    <h1 className="pane-title">Assistant history</h1>
    <p className="card-note">Latest 50 runs, saved only on this Mac, including requests, prompts, answers, and captured screenshots. Older runs and their screenshots are removed automatically.</p>
    <button className="plain" disabled={loading} onClick={() => void refresh()}>Refresh</button>
    {error && <p role="alert">{error}</p>}
    {loading && <p>Loading runs…</p>}
    {!loading && entries.length === 0 && <p>No Assistant runs saved yet.</p>}
    {record && <article className="permission-row" aria-label="Generation record">
      <div className="inline"><h2>Run details</h2><button className="plain" onClick={() => setRecord(null)}>Close details</button></div>
      <p>{new Date(record.createdAt).toLocaleString()} · {record.source} · {record.status}</p>
      {record.message && <p>{record.message}</p>}
      {[ ["Request", record.request], ["Speech transcript", record.transcript], ["Generated answer", record.output] ].map(([label, value]) => value ? <section key={label}><h3>{label}</h3><p style={textStyle}>{value}</p></section> : null)}
      {record.screenshot && <details><summary>Captured screenshot</summary><img src={record.screenshot} alt="Screenshot captured for this Assistant run" style={{ maxWidth: "100%", height: "auto" }} /></details>}
      {[ ["System instructions", record.systemInstructions], ["Model input", record.userPrompt], ["Raw model output (including delivery marker)", record.rawOutput] ].map(([label, value]) => value ? <details key={label}><summary>{label}</summary><pre style={textStyle}>{value}</pre></details> : null)}
      <details><summary>Run timeline</summary><ol>{record.events?.map((event, index) => <li key={index}>{new Date(event.at).toLocaleTimeString()} · {event.phase} · {event.message}</li>)}</ol></details>
    </article>}
    {entries.map(entry => <article className="permission-row" key={entry.id}>
      <div className="card-note">{new Date(entry.createdAt).toLocaleString()} · {entry.source} · {entry.status}</div>
      <p style={textStyle}>{entry.request || "No request transcribed"}</p>
      <button className="plain" onClick={() => void open(entry.id)}>View run</button>
    </article>)}
  </div>;
}
