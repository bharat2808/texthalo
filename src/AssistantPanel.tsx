import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./assistant.css";

type Status = { id?: number; phase?: string; action?: string; message?: string; text?: string; canUndo?: boolean };
export default function AssistantPanel() {
  const [status, setStatus] = useState<Status>({});
  const current = useRef(status);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const busy = ["recording", "processing", "capturing_screen"].includes(status.phase ?? "");
  const applied = status.action === "applied";
  async function action(command: string) {
    setError(""); setPending(true);
    try { await invoke("assistant_control", { command, requestId: current.current.id }); }
    catch (cause) { setError(String(cause)); }
    finally { setPending(false); }
  }
  useEffect(() => {
    let disposed = false;
    let received = false;
    const update = (value: Status) => { if (!disposed) { current.current = value; setStatus(value); setError(""); } };
    const off = listen<Status>("texthalo:assistant", ({ payload }) => { received = true; update(payload ?? {}); });
    void off.then(() => invoke<{ status: Status }>("assistant_state")).then(value => { if (!received) update(value.status ?? {}); }).catch(cause => { if (!disposed) setError(String(cause)); });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); void action("cancel"); }
    };
    window.addEventListener("keydown", onKey);
    return () => { disposed = true; void off.then(unlisten => unlisten()); window.removeEventListener("keydown", onKey); };
  }, []);
  return <main className="assistant-panel">
    <header><span className="assistant-eyebrow">TEXTHALO</span><button aria-label="Dismiss" onClick={() => void action("cancel")}>×</button></header>
    <div className="assistant-status-row"><span className={busy ? "assistant-indicator active" : "assistant-indicator"} aria-hidden="true" />
      <p role="status">{status.message || "Ready"}</p>
      {applied && status.canUndo && <button disabled={pending} onClick={() => void action("undo")}>Undo</button>}
    </div>
    {error && <p role="alert" className="assistant-error">{error}</p>}
    {!busy && !applied && status.text && <><div className="assistant-result" aria-label="Generated text" tabIndex={0}>{status.text}</div>
      <div className="assistant-actions"><button disabled={pending} onClick={() => void action("copy")}>Copy</button></div></>}
    {busy && <span className="assistant-help">Esc to cancel</span>}
  </main>;
}
