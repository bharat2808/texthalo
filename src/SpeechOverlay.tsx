import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./SpeechOverlay.css";

type Status = { phase: "idle" | "capturing" | "preparing" | "speaking" | "error"; message?: string | null };
type AssistantStatus = { id?: number; mode?: string; phase?: string; provider?: string; message?: string };

export default function SpeechOverlay() {
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status>({ phase: "idle" });
  const [assistantErrorStatus, setAssistantErrorStatus] = useState<AssistantStatus | null>(null);
  useEffect(() => {
    let mounted = true;
    const refresh = () => invoke<Status>("get_speech_status").then((next) => {
      if (mounted) setStatus(next);
    }).catch(() => {});
    const subscription = listen<Status>("kiegen:status", ({ payload }) => setStatus(payload));
    void refresh();
    // The panel is created hidden: a snapshot also covers events before it mounted.
    const timer = window.setInterval(() => void refresh(), 250);
    return () => { mounted = false; window.clearInterval(timer); void subscription.then((off) => off()); };
  }, []);
  useEffect(() => { setActionError(null); }, [status.phase, status.message]);
  const recover = async (action: string, assistantRequestId?: number) => {
    setBusy(true);
    setActionError(null);
    try {
      if (assistantRequestId !== undefined) await invoke("assistant_control", { command: "cancel", requestId: assistantRequestId });
      await invoke("speech_error_recovery", { action });
    }
    catch (cause) { setActionError(String(cause)); }
    finally { setBusy(false); }
  };
  useEffect(() => {
    let mounted = true;
    const update = (value: AssistantStatus) => setAssistantErrorStatus(
      value.phase === "error" && value.mode === "assistant" ? value : null,
    );
    const subscription = listen<AssistantStatus>("texthalo:assistant", ({ payload }) => update(payload ?? {}));
    void invoke<{status: AssistantStatus}>("assistant_state").then(value => { if (mounted) update(value.status ?? {}); }).catch(() => {});
    return () => { mounted = false; void subscription.then(off => off()); };
  }, []);
  useEffect(() => { setActionError(null); }, [assistantErrorStatus?.id, assistantErrorStatus?.message]);
  if (status.phase === "error" || assistantErrorStatus) {
    const assistantError = assistantErrorStatus !== null;
    const message = assistantErrorStatus?.message || status.message || "Something went wrong while preparing speech. Try again or review Voice settings.";
    const signIn = /sign[ -]in/i.test(message);
    const billing = /credits|creator plan|subscription|entitlement/i.test(message);
    const local = !assistantError && (signIn || billing || /hosted|fish|switch to the apple/i.test(message));
    const action = assistantError ? signIn ? "account" : billing ? "billing" : "assistant" : signIn ? "account" : billing ? "billing" : "voice";
    const actionLabel = signIn ? (/expired|renewal|again/i.test(message) ? "Sign in again" : "Sign in") : billing ? "Open Billing" : assistantError ? "Assistant settings" : "Voice settings";
    return (
      <div className="overlay-root speech-overlay error error-panel">
        <div className="error-heading"><span className="activity" aria-hidden="true">!</span><strong>{assistantError ? "Unable to answer" : "Unable to speak"}</strong>
          <button className="dismiss" aria-label={assistantError ? "Dismiss Assistant error" : "Dismiss speech error"} title="Dismiss" onClick={() => assistantError ? void invoke("assistant_control", { command: "cancel", requestId: assistantErrorStatus?.id }) : void invoke("stop_speaking")}>×</button>
        </div>
        <p className="error-reason" role="alert">{message}</p>
        {actionError && <p className="recovery-error" role="alert">{actionError}</p>}
        <div className="recovery-actions">
          <button disabled={busy} onClick={() => assistantError
            ? void recover(action, assistantErrorStatus?.id)
            : void recover(action)}>{actionLabel}</button>
          {local && <button className="secondary" disabled={busy} onClick={() => void recover("local")}>Use local voice</button>}
        </div>
      </div>
    );
  }
  if (status.phase === "idle") return <div className="overlay-root" />;
  const label = status.phase === "capturing" ? "Reading selection…"
    : status.phase === "preparing" ? "Preparing speech…"
    : "Speaking";
  return (
    <div className={`overlay-root speech-overlay ${status.phase}`}>
      <div className="activity" aria-hidden="true">
        {[0, 1, 2, 3, 4].map((i) => <i key={i} style={{ animationDelay: `${i * -0.13}s` }} />)}
      </div>
      <span className="speech-label" role="status" title={status.message ?? label}>{label}</span>
      <button aria-label="Stop speech"
        title="Stop speech (⌘⇧X)"
        onClick={() => void invoke("stop_speaking")}>
        <span />
      </button>
    </div>
  );
}
