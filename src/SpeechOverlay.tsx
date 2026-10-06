import { t, useLanguage } from "./i18n";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SpeechErrorPanel from "./SpeechErrorPanel";
import OverlayGrip from "./OverlayGrip";
import "./SpeechOverlay.css";

type Status = { phase: "idle" | "capturing" | "preparing" | "speaking" | "error"; message?: string | null };
type AssistantStatus = { id?: number; mode?: string; phase?: string; provider?: string; message?: string; text?: string; canUndo?: boolean };

export default function SpeechOverlay() {
  useLanguage();
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
  const assistantControl = async (command: string) => {
    setBusy(true);
    setActionError(null);
    try { await invoke("assistant_control", { command, requestId: assistantErrorStatus?.id }); }
    catch (cause) { setActionError(String(cause)); }
    finally { setBusy(false); }
  };
  useEffect(() => {
    let mounted = true;
    let received = false;
    const update = (value: AssistantStatus) => { if (mounted) setAssistantErrorStatus(
      value.phase === "error" && value.mode === "assistant" ? value : null,
    ); };
    const subscription = listen<AssistantStatus>("texthalo:assistant", ({ payload }) => { received = true; update(payload ?? {}); });
    void subscription.then(() => invoke<{status: AssistantStatus}>("assistant_state")).then(value => { if (!received) update(value.status ?? {}); }).catch(() => {});
    return () => { mounted = false; void subscription.then(off => off()); };
  }, []);
  useEffect(() => { setActionError(null); }, [assistantErrorStatus?.id, assistantErrorStatus?.message]);
  if (status.phase === "error" || assistantErrorStatus) {
    const assistantError = assistantErrorStatus !== null;
    const message = assistantErrorStatus?.message || status.message || t("Something went wrong while preparing speech. Try again or review Voice settings.");
    const signIn = /sign[ -]in/i.test(message);
    const billing = /credits|creator plan|subscription|entitlement/i.test(message);
    const local = !assistantError && (signIn || billing || /hosted|fish|switch to the apple/i.test(message));
    const action = assistantError ? signIn ? "account" : billing ? "billing" : "assistant" : signIn ? "account" : billing ? "billing" : "voice";
    const actionLabel = signIn ? (/expired|renewal|again/i.test(message) ? t("Sign in again") : t("Sign in")) : billing ? t("Open Billing") : assistantError ? t("Assistant settings") : t("Voice settings");
    return <SpeechErrorPanel assistant={assistantError} message={message} actionError={actionError} busy={busy} actionLabel={actionLabel}
      dismiss={() => assistantError ? void assistantControl("cancel") : void invoke("stop_speaking")}
      recover={() => void recover(action, assistantErrorStatus?.id)}
      useLocal={local ? () => void recover("local") : undefined}
      textRecovery={assistantErrorStatus?.text ? { text: assistantErrorStatus.text, canUndo: assistantErrorStatus.canUndo,
        copy: () => void assistantControl("copy"), undo: () => void assistantControl("undo") } : undefined} />;
  }
  if (status.phase === "idle") return <div className="overlay-root" />;
  const label = status.phase === "capturing" ? t("Reading selection…")
    : status.phase === "preparing" ? t("Preparing speech…")
    : t("Speaking");
  return (
    <div className={`overlay-root speech-overlay ${status.phase}`}>
      <OverlayGrip onError={setActionError}><div className="activity" aria-hidden="true">
        {[0, 1, 2, 3, 4].map((i) => <i key={i} style={{ animationDelay: `${i * -0.13}s` }} />)}
      </div></OverlayGrip>
      <span className="speech-label" role="status" title={status.message ?? label}>{label}</span>
      <button aria-label={t("Stop speech")}
        title={t("Stop speech (⌘⇧X)")}
        onClick={() => void invoke("stop_speaking")}>
        <span />
      </button>
    </div>
  );
}
