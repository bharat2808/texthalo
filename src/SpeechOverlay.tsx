import { t, useLanguage } from "./i18n";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SpeechErrorPanel from "./SpeechErrorPanel";
import OverlayGrip from "./OverlayGrip";
import "./SpeechOverlay.css";

type Status = { phase: "idle" | "capturing" | "preparing" | "speaking" | "error"; message?: string | null };
type Playback = { position: number; buffered: number; duration: number | null; paused: boolean; speed: number; available: boolean };
const emptyPlayback: Playback = { position: 0, buffered: 0, duration: null, paused: false, speed: 1, available: false };
const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
type AssistantStatus = { id?: number; mode?: string; phase?: string; provider?: string; message?: string; text?: string; canUndo?: boolean };

export default function SpeechOverlay() {
  useLanguage();
  const [actionError, setActionError] = useState<string | null>(null);
  const [playback, setPlayback] = useState<Playback>(emptyPlayback);
  const [expanded, setExpanded] = useState(false);
  const [interacting, setInteracting] = useState(false);
  const collapseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pointerInside = useRef(false);
  const focusInside = useRef(false);
  const showControls = expanded || playback.paused;
  const interaction = () => {
    const active = pointerInside.current || focusInside.current;
    setInteracting(active);
    if (collapseTimer.current) clearTimeout(collapseTimer.current);
    if (active) setExpanded(true);
    else collapseTimer.current = setTimeout(() => setExpanded(false), 1500);
  };
  useEffect(() => () => { if (collapseTimer.current) clearTimeout(collapseTimer.current); }, []);
  useEffect(() => { void invoke("set_playback_overlay", { expanded: showControls, interacting }).catch(() => {}); }, [showControls, interacting]);
  useEffect(() => {
    let mounted = true;
    let received = false;
    const update = (hovered: boolean) => {
      if (!mounted || pointerInside.current === hovered) return;
      pointerInside.current = hovered;
      // A control can be removed by Replay, so do not retain its stale focus flag.
      focusInside.current = !!document.activeElement?.closest(".playback-overlay");
      interaction();
    };
    const subscription = listen<boolean>("texthalo:playback-hover", ({ payload }) => {
      received = true;
      update(payload);
    });
    void subscription.then(() => invoke<boolean>("get_playback_hover"))
      .then(hovered => { if (!received && typeof hovered === "boolean") update(hovered); }).catch(() => {});
    return () => { mounted = false; void subscription.then(off => off()); };
  }, []);
  const control = async (command: string, value?: number) => {
    setActionError(null);
    try {
      await invoke("playback_control", { command, value: value ?? null });
      setPlayback(await invoke<Playback>("get_playback_status"));
    } catch (cause) { setActionError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status>({ phase: "idle" });
  const [assistantErrorStatus, setAssistantErrorStatus] = useState<AssistantStatus | null>(null);
  useEffect(() => {
    let mounted = true;
    const refresh = () => Promise.all([invoke<Status>("get_speech_status"), invoke<Playback | null>("get_playback_status")]).then(([next, audio]) => {
      if (mounted) { setStatus(next); setPlayback(audio ?? emptyPlayback); }
    }).catch(() => {});
    const subscription = listen<Status>("kiegen:status", ({ payload }) => setStatus(payload));
    void refresh();
    // The panel is created hidden: a snapshot also covers events before it mounted.
    const timer = window.setInterval(() => void refresh(), 250);
    return () => { mounted = false; window.clearInterval(timer); void subscription.then((off) => off()); };
  }, []);
  useEffect(() => { setActionError(null); }, [status.phase, status.message]);
  useEffect(() => {
    if (status.phase === "idle" && !playback.available) {
      setExpanded(false); setInteracting(false);
      pointerInside.current = false; focusInside.current = false;
    }
  }, [status.phase, playback.available]);
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
  if (status.phase === "idle" && !playback.available) return <div className="overlay-root" />;
  const completed = status.phase === "idle" && playback.available;
  const label = playback.paused ? t("Paused") : completed ? t("Finished") : status.phase === "capturing" ? t("Reading selection…")
    : status.phase === "preparing" ? t("Preparing speech…")
    : t("Speaking");
  return (
    <div className={`overlay-root speech-overlay playback-overlay ${status.phase} ${showControls ? "controls-expanded" : ""} ${playback.paused ? "paused" : ""}`}
      onMouseEnter={() => { pointerInside.current = true; interaction(); }}
      onMouseLeave={event => { pointerInside.current = false; focusInside.current = event.currentTarget.contains(document.activeElement); interaction(); }}
      onFocus={() => { focusInside.current = true; interaction(); }}
      onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) { focusInside.current = false; interaction(); } }}>
      <div className="playback-heading">
        <OverlayGrip onError={setActionError}><div className="activity" aria-hidden="true">
          {[0, 1, 2, 3, 4].map((i) => <i key={i} style={{ animationDelay: `${i * -0.13}s` }} />)}
        </div></OverlayGrip>
        <span className="speech-label" role={actionError ? "alert" : "status"} title={actionError ?? status.message ?? label}>{actionError ?? label}</span>
        {completed ? <button className="playback-button replay" aria-label={t("Replay")} title={t("Replay")} onClick={() => void control("replay")}>↻</button> : null}
        <button className="playback-stop" aria-label={t("Stop speech")}
          title={t("Stop speech (⌘⇧X)")} onClick={() => void invoke("stop_speaking").catch(cause => setActionError(String(cause)))}><span /></button>
      </div>
      {showControls ? <div className="playback-controls">
        <div className="playback-actions">
          <button className="playback-button" disabled={!playback.available} aria-label={t("Back 10 seconds")} title={t("Back 10 seconds")} onClick={() => void control("seek", playback.position - 10)}>↶<small>10</small></button>
          {!completed ? <button className="playback-button playback-toggle" disabled={!playback.available} aria-label={playback.paused ? t("Resume") : t("Pause")} title={playback.paused ? t("Resume") : t("Pause")}
            onClick={() => void control(playback.paused ? "resume" : "pause")}>{playback.paused ? "▶" : "Ⅱ"}</button> : null}
          <button className="playback-button" disabled={!playback.available} aria-label={t("Forward 10 seconds")} title={t("Forward 10 seconds")} onClick={() => void control("seek", playback.position + 10)}>↷<small>10</small></button>
          <select className="playback-speed" aria-label={t("Playback speed")} disabled={!playback.available} value={playback.speed || 1} onChange={event => void control("speed", Number(event.target.value))}>
            {[0.5, 0.75, 1, 1.25, 1.5, 1.75, 2].map(speed => <option key={speed} value={speed}>{speed}×</option>)}
          </select>
        </div>
        <div className="playback-progress">
          <span>{clock(playback.position)}</span>
          <input type="range" min="0" max={playback.buffered || 1} step="0.1" value={Math.min(playback.position, playback.buffered)} disabled={!playback.available}
            aria-label={t("Playback position")} aria-valuetext={`${clock(playback.position)} / ${clock(playback.buffered)}`}
            title={playback.duration === null ? t("Seek within generated audio") : t("Playback position")}
            onChange={event => void control("seek", Number(event.target.value))} />
          <span>{clock(playback.duration ?? playback.buffered)}{playback.duration === null ? "+" : ""}</span>
        </div>
      </div> : null}
    </div>
  );
}
