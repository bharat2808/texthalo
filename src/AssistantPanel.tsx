import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./assistant.css";

type Status = { id?: number; phase?: string; message?: string; text?: string; source?: string; context?: string; canInsert?: boolean; hostedSpeech?: boolean };
export default function AssistantPanel() {
  const [status, setStatus] = useState<Status>({});
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState("");
  const [hosted, setHosted] = useState(false);
  const busy = ["recording", "processing", "capturing_screen"].includes(status.phase ?? "");
  async function action(command: string, extras: Record<string, string> = {}) {
    setError("");
    try { await invoke("assistant_control", { command, ...extras }); }
    catch (cause) { setError(String(cause)); }
  }
  useEffect(() => {
    let disposed = false;
    const off = listen<Status>("texthalo:assistant", ({ payload }) => { if (!disposed) { setStatus(payload ?? {}); if (typeof payload?.hostedSpeech === "boolean") setHosted(payload.hostedSpeech); } });
    void invoke<{ status: Status }>("assistant_state").then(value => { if (!disposed) setStatus(value.status ?? {}); }).catch(cause => setError(String(cause)));
    void invoke<{ settings: { engine: string } }>("get_state").then(value => { if (!disposed) setHosted(value.settings.engine === "fish"); }).catch(cause => setError(String(cause)));
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); void action("cancel"); }
    };
    window.addEventListener("keydown", onKey);
    return () => { disposed = true; void off.then(unlisten => unlisten()); window.removeEventListener("keydown", onKey); };
  }, []);
  return <main className="assistant-panel">
    <header><div><span className="assistant-eyebrow">TEXTHALO</span><h1>{status.phase === "recording" ? "Listening" : "Assistant"}</h1></div>
      <button onClick={() => void action("cancel")} aria-label="Cancel and close">×</button></header>
    <p className="assistant-status" role="status">{status.message || "Hold your Assistant shortcut to speak a request."}</p>
    {status.source && <p className="assistant-source">From {status.source}{status.context ? ` · ${status.context.length.toLocaleString()} context characters` : " · No selection context"}</p>}
    {error && <p role="alert" className="assistant-error">{error}</p>}
    {status.phase === "error" && <p className="assistant-help">Microphone, speech permissions, language, and shortcuts are configured in Settings → Shortcuts.</p>}
    {status.text && <><div className="assistant-result" aria-label="Generated text" tabIndex={0}>{status.text}</div>
      <div className="assistant-actions">
        <button disabled={busy} onClick={() => void action("copy")}>Copy</button>
        <button disabled={busy || !status.canInsert} title="Insert or replace only in the unchanged original field" onClick={() => void action("insert")}>Insert / Replace</button>
        <button disabled={busy} onClick={() => void action("speak")}>Speak</button>
      </div></>}
    <p className="assistant-privacy">{hosted ? "Generation stays on this Mac. Speaking sends text to the hosted Fish voice service." : "Transcription, generation, and your selected voice run on this Mac."}</p>
    {!busy && <>
      <form onSubmit={event => { event.preventDefault(); if (prompt.trim()) { void action("ask", { text: prompt }); setPrompt(""); } }}>
        <label htmlFor="assistant-prompt">Request or follow-up</label>
        <textarea id="assistant-prompt" value={prompt} onChange={event => setPrompt(event.target.value)} maxLength={4000} rows={3} placeholder="Summarize this and read it aloud…" />
        <button type="submit" disabled={!prompt.trim()}>Ask assistant</button>
      </form>
      <details><summary>Add screen text</summary><p>Captures only when you choose an option. Local OCR extracts visible text; images and diagrams are not interpreted.</p>
        <div className="assistant-actions"><button onClick={() => void action("screen", { context: "window" })}>Original window</button><button onClick={() => void action("screen", { context: "region" })}>Choose region</button></div>
      </details>
    </>}
    {busy && <button className="assistant-cancel" onClick={() => void action("cancel")}>Cancel · Esc</button>}
  </main>;
}
