import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import SelectionRow from "./SelectionRow";
import type { AssistantPreferences, Availability } from "./AssistantSettings";

type Model = { id: string; name: string; bytes: number; installed: boolean };
type Catalog = { models: Model[]; download: { id: string; received: number; total: number } | null; error: string | null };
const bytes = (value: number) => value >= 1_000_000_000 ? `${(value / 1_000_000_000).toFixed(1)} GB` : `${Math.round(value / 1_000_000)} MB`;

export default function TranscriptionSource({ value, onChange, availability, warningIcon, downloadIcon }: {
  value: AssistantPreferences; onChange: (value: AssistantPreferences) => void;
  availability: Availability; warningIcon: ReactNode; downloadIcon: ReactNode;
}) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [canceling, setCanceling] = useState(false);
  const engine = value.transcription_engine ?? "apple";
  const selected = catalog?.models.find(model => model.id === (value.whisper_model ?? "small"));
  useEffect(() => {
    let alive = true;
    const refresh = () => { void invoke<Catalog>("whisper_models").then(result => {
      if (alive) setCatalog(result);
    }).catch(cause => { if (alive) setError(String(cause)); }); };
    refresh();
    const interval = window.setInterval(refresh, 500);
    return () => { alive = false; window.clearInterval(interval); };
  }, []);
  function selectSource(source: "apple" | "whisper") {
    const base = value.language.split(/[-_]/)[0];
    const languages = source === "apple" ? availability.speechLanguages : availability.whisperLanguages;
    const language = source === "apple"
      ? languages?.find(language => language.code === value.language)?.code
        ?? languages?.find(language => language.code === navigator.language && language.code.split(/[-_]/)[0] === base)?.code
        ?? languages?.find(language => language.code.split(/[-_]/)[0] === base)?.code
        ?? "en-US"
      : value.language === "auto" || languages?.some(language => language.code === base) ? value.language : "auto";
    onChange({ ...value, transcription_engine: source, language });
  }
  async function command(name: string) {
    setBusy(true); setError(""); setCanceling(false);
    try { await invoke(name, { id: selected?.id }); }
    catch (cause) { setError(String(cause)); }
    finally {
      try { setCatalog(await invoke<Catalog>("whisper_models")); } catch (cause) { setError(String(cause)); }
      setBusy(false); setCanceling(false);
    }
  }
  const download = catalog?.download;
  useEffect(() => { if (!download) setCanceling(false); }, [download]);
  const percent = download ? Math.min(100, Math.floor(download.received / download.total * 100)) : 0;
  return <>
    <div className="row-stack" role="group" aria-label="Transcription source">
      <SelectionRow selected={engine === "apple"} title="macOS speech recognition" subtitle="On this Mac · no download"
        badge={availability.recognition === 3 ? "Ready" : "Not ready"}
        onSelect={() => selectSource("apple")} />
      <SelectionRow selected={engine === "whisper"} title="Whisper" subtitle="On this Mac · free · multilingual"
        badge={selected?.installed ? "Ready" : selected ? `Needs ${bytes(selected.bytes)}` : "Checking…"}
        onSelect={() => selectSource("whisper")} />
    </div>
    {engine === "whisper" && <>
      {selected && !selected.installed && <div className="note-line warning">{warningIcon}<span>Download {selected.name} to use Whisper.</span></div>}
      <div className="field">
        <label className="field-label" htmlFor="whisper-model">Model size</label>
        <select id="whisper-model" value={value.whisper_model ?? "small"} disabled={!catalog || busy || !!download}
          onChange={event => onChange({ ...value, whisper_model: event.target.value, language: value.language.split(/[-_]/)[0] === "yue" && !event.target.value.startsWith("large-v3") ? "auto" : value.language })}>
          {!catalog && <option value={value.whisper_model ?? "small"}>Loading models…</option>}
          {catalog?.models.map(model => <option key={model.id} value={model.id}>{model.name}{model.id === "small" ? " (Recommended)" : ""} · {bytes(model.bytes)}{model.installed ? " · Downloaded" : ""}</option>)}
        </select>
        <p className="card-note">Small balances speed and accuracy. Larger models need more memory and may take longer. Audio stays on this Mac. Also used by Assistant.</p>
      </div>
      {(download || selected && !selected.installed) && <div className="field">
        <span className="field-label">Model download</span>
        {download ? <>
          <div className="progress"><div className="progress-bar" style={{ width: `${percent}%` }} role="progressbar" aria-label="Whisper model download" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100} /></div>
          <span className="field-hint">{percent}% · {bytes(download.received)} of {bytes(download.total)}{percent === 100 ? " · Verifying…" : ""}</span>
          <div className="inline"><button className="plain" disabled={canceling} onClick={() => {
            setCanceling(true); void invoke("whisper_cancel_download").catch(cause => { setError(String(cause)); setCanceling(false); });
          }}>{canceling ? "Canceling…" : "Cancel download"}</button></div>
        </> : <div className="inline"><button className="plain" disabled={!selected || busy} onClick={() => void command("whisper_download")}>{downloadIcon} {busy ? "Starting…" : `Download ${bytes(selected?.bytes ?? 0)}`}</button></div>}
      </div>}
      {selected?.installed && <details className="disclosure"><summary>Advanced model settings</summary>
        <p className="card-note">Whisper stays ready in memory between requests and unloads after five idle minutes.</p>
        <div className="inline"><button className="plain" disabled={busy || !!download} onClick={() => void command("whisper_remove")}>Delete {selected.name} download</button></div>
      </details>}
    </>}
    {error || catalog?.error ? <div className="note-line error" role="alert">{warningIcon}<span>{error || catalog?.error}</span></div> : null}
  </>;
}
