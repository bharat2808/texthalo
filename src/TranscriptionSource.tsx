import { t, message } from "./i18n";
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
    <div className="row-stack" role="group" aria-label={t("Transcription source")}>
      <SelectionRow selected={engine === "apple"} title={t("macOS speech recognition")} subtitle={t("On this Mac · no download")}
        badge={availability.recognition === 3 ? t("Ready") : t("Not ready")}
        onSelect={() => selectSource("apple")} />
      <SelectionRow selected={engine === "whisper"} title="Whisper" subtitle={t("On this Mac · free · multilingual")}
        badge={selected?.installed ? t("Ready") : selected ? t("Needs {0}", [bytes(selected.bytes)]) : t("Checking…")}
        onSelect={() => selectSource("whisper")} />
    </div>
    {engine === "whisper" && <>
      {selected && !selected.installed && <div className="note-line warning">{warningIcon}<span>{t("Download")} {selected.name}  {t("to use Whisper.")}</span></div>}
      <div className="field">
        <label className="field-label" htmlFor="whisper-model">{t("Model size")}</label>
        <select id="whisper-model" value={value.whisper_model ?? "small"} disabled={!catalog || busy || !!download}
          onChange={event => onChange({ ...value, whisper_model: event.target.value, language: value.language.split(/[-_]/)[0] === "yue" && !event.target.value.startsWith("large-v3") ? "auto" : value.language })}>
          {!catalog && <option value={value.whisper_model ?? "small"}>{t("Loading models…")}</option>}
          {catalog?.models.map(model => <option key={model.id} value={model.id}>{model.name}{model.id === "small" ? t(" (Recommended)") : ""} · {bytes(model.bytes)}{model.installed ? t(" · Downloaded") : ""}</option>)}
        </select>
        <p className="card-note">{t("Small balances speed and accuracy. Larger models need more memory and may take longer. Audio stays on this Mac. Also used by Assistant.")}</p>
      </div>
      {(download || selected && !selected.installed) && <div className="field">
        <span className="field-label">{t("Model download")}</span>
        {download ? <>
          <div className="progress"><div className="progress-bar" style={{ width: `${percent}%` }} role="progressbar" aria-label={t("Whisper model download")} aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100} /></div>
          <span className="field-hint">{percent}% · {bytes(download.received)}  {t("of")} {bytes(download.total)}{percent === 100 ? t(" · Verifying…") : ""}</span>
          <div className="inline"><button className="plain" disabled={canceling} onClick={() => {
            setCanceling(true); void invoke("whisper_cancel_download").catch(cause => { setError(String(cause)); setCanceling(false); });
          }}>{canceling ? t("Canceling…") : t("Cancel download")}</button></div>
        </> : <div className="inline"><button className="plain accent-action" disabled={!selected || busy} onClick={() => void command("whisper_download")}>{downloadIcon} {busy ? t("Starting…") : t("Download {0}", [bytes(selected?.bytes ?? 0)])}</button></div>}
      </div>}
      {selected?.installed && <details className="disclosure"><summary>{t("Advanced model settings")}</summary>
        <p className="card-note">{t("Whisper stays ready in memory between requests and unloads after five idle minutes.")}</p>
        <div className="inline"><button className="plain accent-action" disabled={busy || !!download} onClick={() => void command("whisper_remove")}>{t("Delete")} {selected.name}  {t("download")}</button></div>
      </details>}
    </>}
    {error || catalog?.error ? <div className="note-line error" role="alert">{warningIcon}<span>{message(error || catalog?.error || "")}</span></div> : null}
  </>;
}
