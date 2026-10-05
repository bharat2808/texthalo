import { useEffect, useRef, useState, type ReactNode } from "react";
import appLogo from "../src-tauri/icons/icon.png";
import KokoroModelPicker, { type KokoroModelOption } from "./KokoroModelPicker";
import "./SetupWizard.css";

type WizardSettings = {
  onboarding_completed: boolean;
  launch_at_login: boolean;
  accessibility_prompted: boolean;
  engine: "apple" | "kokoro" | "chatterbox" | "fish";
  shortcuts: { speak: string; stop: string; transcribe: string; assistant: string };
  voice: string | null;
  kokoro: { voice: string; quant: string; speed: number; keep_warm: boolean; idle_unload_minutes: number };
  chatterbox: { voice: string; quant: string; exaggeration: number; cfg_weight: number; ref_audio: string | null; keep_warm: boolean };
};
type WizardVoice = { name: string; locale: string; novelty: boolean };
type WizardEngine = {
  id: WizardSettings["engine"]; label: string; summary: string; status: string;
  can_speak: boolean; needs_download: boolean; download_bytes: number;
  voices: { id: string; label: string; language: string; unavailable: string | null }[];
  ref_voices: { id: string; label: string; builtin: boolean }[];
  models: KokoroModelOption[];
};
type WizardInstall = {
  engine: WizardSettings["engine"]; quant?: string | null;
  phase: "downloading" | "done" | "error"; done: number; total: number; message: string | null;
};
export type SetupDestination = "home" | "voice" | "general";
type SetupPlan = {
  id: string; name: string; description: string;
  price: { unitAmount: number; currency: string; interval: string | null; intervalCount?: number } | null;
  features: string[];
};
export type SetupPlansResponse = { billingEnabled: boolean; plans: SetupPlan[] };

type SetupWizardProps = {
  settings: WizardSettings; voices: WizardVoice[]; engines: WizardEngine[];
  trusted: boolean; externalError: string | null;
  recordingShortcut: keyof WizardSettings["shortcuts"] | null;
  shortcutSaving: boolean;
  speaking: boolean; downloadBusy: boolean; install: WizardInstall | null;
  onSave: (patch: Partial<WizardSettings>) => void | Promise<void>;
  onDownload: () => Promise<void>;
  onRequestAccessibility: () => void;
  onOpenAccessibilitySettings: () => void;
  onPreviewVoice: () => Promise<unknown>;
  onStopPreview: () => void;
  renderShortcut: (role: keyof WizardSettings["shortcuts"]) => ReactNode;
  onFinish: (destination: SetupDestination) => void;
};

const STEPS = ["Voice", "Access", "Shortcuts"];
const SHORTCUTS = [
  { role: "speak", title: "Listen", description: "Read selected text." },
  { role: "stop", title: "Stop", description: "Stop the current reading." },
] as const;
const bytes = (value: number) => value >= 1_000_000_000 ? `${(value / 1_000_000_000).toFixed(2)} GB` : `${Math.round(value / 1_000_000)} MB`;

export default function SetupWizard({ settings, voices, engines, trusted, externalError, recordingShortcut, shortcutSaving,
  speaking, downloadBusy, install, onSave, onDownload, onRequestAccessibility, onOpenAccessibilitySettings,
  onPreviewVoice, onStopPreview, renderShortcut, onFinish,
}: SetupWizardProps) {
  const [step, setStep] = useState(0);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const content = useRef<HTMLElement | null>(null);
  const active = engines.find(engine => engine.id === settings.engine);
  const local = settings.engine === "kokoro" || settings.engine === "chatterbox";
  const modelId = settings.engine === "kokoro" ? settings.kokoro.quant : settings.chatterbox.quant;
  const needsDownload = Boolean(active?.needs_download || active?.models?.find(model => model.id === modelId)?.installed === false);
  const progress = install?.engine === settings.engine && (!install.quant || install.quant === modelId) ? install : null;
  const percent = progress?.total ? Math.min(100, Math.round(progress.done / progress.total * 100)) : 0;
  const busy = saving || previewing || finishing || shortcutSaving || recordingShortcut !== null;
  const selectedVoice = settings.engine === "kokoro" ? settings.kokoro.voice : settings.chatterbox.voice;
  const unavailableVoice = local && active?.voices.find(voice => voice.id === selectedVoice)?.unavailable;

  useEffect(() => { content.current?.scrollTo({ top: 0 }); }, [step]);
  useEffect(() => { if (needsDownload) setAdvancedOpen(true); }, [needsDownload, settings.engine, modelId]);

  async function update(patch: Partial<WizardSettings>) {
    setError(""); setSaving(true);
    try { await onSave(patch); } catch (cause) { setError(String(cause)); }
    finally { setSaving(false); }
  }
  function next() { setError(""); setStep(current => Math.min(current + 1, STEPS.length - 1)); }
  async function preview() {
    setError(""); setPreviewing(true);
    try { await onPreviewVoice(); } catch (cause) { setError(String(cause)); }
    finally { setPreviewing(false); }
  }
  async function download() {
    setError("");
    try { await onDownload(); } catch (cause) { setError(String(cause)); }
  }
  async function finish(destination: SetupDestination = "home") {
    setError(""); setFinishing(true);
    try { await onSave({ onboarding_completed: true }); onFinish(destination); }
    catch (cause) { setError(String(cause)); }
    finally { setFinishing(false); }
  }

  return <main className="setup-wizard">
    <header className="setup-wizard-header">
      <img className="setup-wizard-mark" src={appLogo} alt="" /><span>Welcome to TextHalo</span>
      <button className="plain" disabled={busy} onClick={() => void finish()}>Skip setup</button>
    </header>
    <div className="setup-wizard-progress" aria-label={`Step ${step + 1} of ${STEPS.length}`}>
      {STEPS.map((label, index) => <div key={label} className={index <= step ? "complete" : ""} aria-current={index === step ? "step" : undefined}><span>{index + 1}</span>{label}</div>)}
    </div>
    <section ref={content} className="setup-wizard-content" aria-live="polite">
      {step === 0 ? <>
        <h1>Choose a voice</h1>
        <label className="setup-field"><span>Voice source</span>
          <select aria-label="Voice source" value={settings.engine} disabled={busy || downloadBusy}
            onChange={event => void update({ engine: event.target.value as WizardSettings["engine"] })}>
            <optgroup label="Local">
              {engines.filter(engine => engine.id !== "fish").map(engine => <option key={engine.id} value={engine.id}>{engine.label}</option>)}
            </optgroup>
            <optgroup label="Cloud">
              {engines.filter(engine => engine.id === "fish").map(engine => <option key={engine.id} value={engine.id}>{engine.label}</option>)}
            </optgroup>
          </select>
        </label>
        <p className="setup-hint">{settings.engine === "fish" ? "Text is sent to the cloud to generate audio. Requires sign-in and credits." : settings.engine === "apple" ? "Audio stays on your Mac. No download needed." : needsDownload ? "Audio is generated on your Mac. Model download required." : "Audio is generated on your Mac. Model ready."}</p>
        {settings.engine === "apple" ? <label className="setup-field"><span>Voice</span>
          <select value={settings.voice ?? ""} disabled={busy} onChange={event => void update({ voice: event.target.value || null })}>
            <option value="">System default</option>
            {voices.filter(voice => !voice.novelty).map(voice => <option key={voice.name} value={voice.name}>{voice.name} · {voice.locale.replace("_", "-")}</option>)}
          </select>
        </label> : null}
        {local ? <>
          <label className="setup-field"><span>{settings.engine === "chatterbox" ? "Language" : "Voice"}</span>
            <select value={selectedVoice} disabled={busy || downloadBusy} onChange={event => void update(settings.engine === "kokoro"
              ? { kokoro: { ...settings.kokoro, voice: event.target.value } }
              : { chatterbox: { ...settings.chatterbox, voice: event.target.value } })}>
              {active?.voices.map(voice => <option key={voice.id} value={voice.id} disabled={Boolean(voice.unavailable)}>{voice.label}{voice.unavailable ? ` · ${voice.unavailable}` : ""}</option>)}
            </select>
          </label>
          {settings.engine === "chatterbox" ? <label className="setup-field"><span>Reference voice</span>
            <select value={settings.chatterbox.ref_audio ?? ""} disabled={busy || downloadBusy} onChange={event => void update({ chatterbox: { ...settings.chatterbox, ref_audio: event.target.value || null } })}>
              <option value="">Built-in voice</option>
              {active?.ref_voices.filter(voice => !voice.builtin).map(voice => <option key={voice.id} value={voice.id}>{voice.label}</option>)}
            </select>
          </label> : null}
          {needsDownload ? <p>Weights missing — download from below.</p> : null}
          <details className="disclosure setup-advanced" open={advancedOpen} onToggle={event => setAdvancedOpen(event.currentTarget.open)}>
            <summary>Advanced</summary>
            <KokoroModelPicker engine={settings.engine === "kokoro" ? "Kokoro" : "Chatterbox"} models={active?.models ?? []}
              selected={modelId} disabled={busy || downloadBusy} needsDownload={needsDownload}
              downloadLabel={progress?.phase === "downloading" ? "Downloading…" : "Download selected model"}
              onDownload={() => void download()} onSelect={quant => void update(settings.engine === "kokoro"
                ? { kokoro: { ...settings.kokoro, quant } } : { chatterbox: { ...settings.chatterbox, quant } })} />
            <p>Uses more memory for faster playback.</p>
            <label className="toggle-row"><input type="checkbox" disabled={busy} checked={settings.engine === "kokoro" ? settings.kokoro.keep_warm : settings.chatterbox.keep_warm}
              onChange={event => void update(settings.engine === "kokoro" ? { kokoro: { ...settings.kokoro, keep_warm: event.target.checked } } : { chatterbox: { ...settings.chatterbox, keep_warm: event.target.checked } })} /><span>Keep {settings.engine === "kokoro" ? "Kokoro" : "Chatterbox"} ready in memory</span></label>
          </details>
          {progress?.phase === "downloading" ? <div className="setup-download-status">
            <div className="progress"><div className="progress-bar" style={{ width: `${percent}%` }} role="progressbar" aria-label="Voice model download" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100} /></div>
            <span>{percent}% · {bytes(progress.done)} of {bytes(progress.total)}</span>
          </div> : null}
          {progress?.phase === "error" ? <p role="alert">{progress.message}</p> : null}
          {unavailableVoice ? <p role="status">{unavailableVoice}</p> : null}
        </> : null}
        {settings.engine === "fish" ? <>
          <button className="plain" disabled={busy || downloadBusy} onClick={() => void finish("voice")}>Set up cloud voices</button>
        </> : null}
        <div className="inline">
          <button className="primary-button" disabled={busy || !active?.can_speak || Boolean(unavailableVoice)} onClick={() => void preview()}>{previewing ? "Starting…" : "Preview voice"}</button>
          {speaking ? <button className="plain" onClick={onStopPreview}>Stop preview</button> : null}
          {settings.engine !== "apple" && !active?.can_speak ? <button className="plain" disabled={busy || downloadBusy} onClick={() => void update({ engine: "apple" })}>Use Mac voice for now</button> : null}
        </div>
      </> : null}
      {step === 1 ? <>
        <h1>Allow text access</h1>
        <p>Enable Accessibility to read selected text from other apps.</p>
        {trusted ? <div className="setup-permission-state granted">Accessibility enabled.</div> : <>
          <button className="primary-button" disabled={busy} onClick={settings.accessibility_prompted ? onOpenAccessibilitySettings : onRequestAccessibility}>Enable Accessibility</button>
          <p>In System Settings, turn on TextHalo under Privacy &amp; Security → Accessibility.</p>
        </>}
      </> : null}
      {step === 2 ? <>
        <h1>Reading shortcuts</h1>
        <div className="setup-shortcuts">
          {SHORTCUTS.map(shortcut => <section className="setup-shortcut-card" key={shortcut.role}>
            <div><strong>{shortcut.title}</strong><p>{shortcut.description}</p></div>
            <div>{renderShortcut(shortcut.role)}</div>
          </section>)}
        </div>
        {!trusted ? <div className="setup-permission-state">Enable Accessibility before reading selected text.<button className="text" disabled={busy} onClick={() => setStep(1)}>Enable access</button></div> : null}
        <div className="setup-later"><span>Set up Dictation or Assistant from Settings.</span><button className="text" disabled={busy} onClick={() => void finish("general")}>Open Settings</button></div>
        <label className="toggle-row setup-launch-toggle"><input type="checkbox" checked={settings.launch_at_login} disabled={busy} onChange={event => void update({ launch_at_login: event.target.checked })} /><span>Open TextHalo when I log in</span></label>
      </> : null}
      {error || externalError ? <p role="alert">{error || externalError}</p> : null}
    </section>
    <footer className="setup-wizard-footer">
      {step > 0 ? <button className="plain" disabled={busy} onClick={() => { setError(""); setStep(current => current - 1); }}>Back</button> : <span />}
      <div className="inline">
        <button className="text" disabled={busy} onClick={() => step === STEPS.length - 1 ? void finish() : next()}>Skip for now</button>
        {step < STEPS.length - 1 ? <button className="primary-button" disabled={busy} onClick={next}>Continue</button>
          : <button className="primary-button" disabled={busy} onClick={() => void finish()}>{finishing ? "Saving…" : "Finish setup"}</button>}
      </div>
    </footer>
  </main>;
}
