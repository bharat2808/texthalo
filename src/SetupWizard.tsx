import { useState } from "react";
import appLogo from "../src-tauri/icons/icon.png";
import "./SetupWizard.css";

type WizardSettings = {
  onboarding_completed: boolean;
  launch_at_login: boolean;
  accessibility_prompted: boolean;
  engine: "apple" | "kokoro" | "chatterbox" | "fish";
  shortcuts: { speak: string; stop: string; transcribe: string; assistant: string };
  voice: string | null;
};

type WizardVoice = { name: string; locale: string; novelty: boolean };
type SetupPlan = {
  id: string;
  name: string;
  description: string;
  price: { unitAmount: number; currency: string; interval: string | null; intervalCount?: number } | null;
  features: string[];
};
export type SetupPlansResponse = { billingEnabled: boolean; plans: SetupPlan[] };

type SetupWizardProps = {
  settings: WizardSettings;
  voices: WizardVoice[];
  trusted: boolean;
  externalError: string | null;
  recordingShortcut: boolean;
  onSave: (patch: Partial<WizardSettings>) => void | Promise<void>;
  onRequestAccessibility: () => void;
  onOpenAccessibilitySettings: () => void;
  onPreviewVoice: (voice: string | null) => Promise<unknown>;
  onRecordShortcut: () => void;
  onFinish: () => void;
};

const STEPS = ["Voice", "Access", "Try it"];

export default function SetupWizard({ settings, voices, trusted, externalError, recordingShortcut, onSave,
  onRequestAccessibility, onOpenAccessibilitySettings, onPreviewVoice, onRecordShortcut, onFinish,
}: SetupWizardProps) {
  const [step, setStep] = useState(0);
  const [error, setError] = useState("");
  const [previewing, setPreviewing] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const speechVoices = voices.filter(voice => !voice.novelty);

  async function update(patch: Partial<WizardSettings>) {
    setError("");
    try { await onSave(patch); } catch (cause) { setError(String(cause)); }
  }

  async function next() {
    setError(""); setFinishing(true);
    try {
      if (step === 0) await onSave({ engine: "apple" });
      setStep(current => current + 1);
    } catch (cause) { setError(String(cause)); }
    finally { setFinishing(false); }
  }

  async function preview() {
    setError(""); setPreviewing(true);
    try { await onSave({ engine: "apple" }); await onPreviewVoice(settings.voice); }
    catch (cause) { setError(String(cause)); }
    finally { setPreviewing(false); }
  }
  async function finish() {
    setError(""); setFinishing(true);
    try { await onSave({ onboarding_completed: true }); onFinish(); }
    catch (cause) { setError(String(cause)); }
    finally { setFinishing(false); }
  }

  return <main className="setup-wizard">
    <header className="setup-wizard-header">
      <img className="setup-wizard-mark" src={appLogo} alt="" /><span>Welcome to TextHalo</span>
      <button className="plain" disabled={finishing || recordingShortcut} onClick={() => void finish()}>Skip setup</button>
    </header>
    <div className="setup-wizard-progress" aria-label={`Step ${step + 1} of ${STEPS.length}`}>
      {STEPS.map((label, index) => <div key={label} className={index <= step ? "complete" : ""} aria-current={index === step ? "step" : undefined}><span>{index + 1}</span>{label}</div>)}
    </div>
    <section className="setup-wizard-content" aria-live="polite">
      {step === 0 ? <>
        <div className="setup-wizard-kicker">MAKE IT SOUND LIKE YOU</div>
        <h1>Find your voice</h1>
        <p>Start with a free voice on your Mac. Explore more voices anytime.</p>
        <label className="setup-field"><span>Voice</span><select value={settings.voice ?? ""} onChange={event => void update({ voice: event.target.value || null, engine: "apple" })}>
          <option value="">System default</option>
          {speechVoices.map(voice => <option key={voice.name} value={voice.name}>{voice.name} · {voice.locale.replace("_", "-")}</option>)}
        </select></label>
        <button className="primary-button" disabled={previewing} onClick={() => void preview()}>{previewing ? "Playing…" : "Preview voice"}</button>
      </> : null}
      {step === 1 ? <>
        <div className="setup-wizard-kicker">ONE QUICK PERMISSION</div>
        <h1>Read from any app</h1>
        <p>Allow Accessibility so TextHalo can read text you select. It only captures text when you use a shortcut.</p>
        {trusted ? <div className="setup-permission-state granted">Accessibility is on. You’re ready.</div> : <>
          <button className="primary-button" onClick={settings.accessibility_prompted ? onOpenAccessibilitySettings : onRequestAccessibility}>Enable Accessibility</button>
          <p>In System Settings, turn on TextHalo under Privacy &amp; Security → Accessibility.</p>
        </>}
      </> : null}
      {step === 2 ? <>
        <div className="setup-wizard-kicker">GIVE IT A GO</div>
        <h1>Try your shortcut</h1>
        <p>Select a sentence in another app, then press:</p>
        <div className="setup-shortcut-display">{recordingShortcut ? "Press your shortcut… Esc to cancel" : settings.shortcuts.speak || "No shortcut set"}</div>
        <button className="plain" disabled={recordingShortcut} onClick={onRecordShortcut}>Change shortcut</button>
        <p>Stop listening with <strong>{settings.shortcuts.stop || "the Stop button"}</strong>.</p>
        {!trusted ? <div className="setup-permission-state">Enable Accessibility before trying your shortcut.<button className="text" onClick={() => setStep(1)}>Enable access</button></div> : null}
        <label className="toggle-row setup-launch-toggle"><input type="checkbox" checked={settings.launch_at_login} onChange={event => void update({ launch_at_login: event.target.checked })} /><span>Open TextHalo when I log in</span></label>
      </> : null}
      {error || externalError ? <p role="alert">{error || externalError}</p> : null}
    </section>
    <footer className="setup-wizard-footer">
      {step > 0 ? <button className="plain" disabled={recordingShortcut || finishing} onClick={() => setStep(current => current - 1)}>Back</button> : <span />}
      {step < STEPS.length - 1 ? <button className="primary-button" disabled={finishing} onClick={() => void next()}>{step === 1 && !trusted ? "Continue without access" : "Continue"}</button>
        : <button className="primary-button" disabled={recordingShortcut || finishing} onClick={() => void finish()}>{finishing ? "Saving…" : "Finish setup"}</button>}
    </footer>
  </main>;
}
