import { useState } from "react";
import "./SetupWizard.css";

type WizardSettings = {
  onboarding_completed: boolean;
  accessibility_prompted: boolean;
  shortcuts: { speak: string; stop: string };
  voice: string | null;
};

type WizardVoice = { name: string; locale: string; novelty: boolean };

type SetupWizardProps = {
  settings: WizardSettings;
  voices: WizardVoice[];
  trusted: boolean;
  recordingShortcut: boolean;
  onSave: (patch: Partial<WizardSettings>) => void | Promise<void>;
  onRequestAccessibility: () => void;
  onOpenAccessibilitySettings: () => void;
  onPreviewVoice: (voice: string | null) => void;
  onRecordShortcut: () => void;
  onFinish: () => void | Promise<void>;
};

const STEPS = ["Access", "Voice", "Shortcut", "Ready"];

export default function SetupWizard({
  settings,
  voices,
  trusted,
  recordingShortcut,
  onSave,
  onRequestAccessibility,
  onOpenAccessibilitySettings,
  onPreviewVoice,
  onRecordShortcut,
  onFinish,
}: SetupWizardProps) {
  const [step, setStep] = useState(0);
  const speechVoices = voices.filter((voice) => !voice.novelty);

  const finish = async () => {
    await onSave({ onboarding_completed: true });
    await onFinish();
  };

  return (
    <main className="setup-wizard">
      <header className="setup-wizard-header">
        <div className="setup-wizard-mark" aria-hidden="true">◖</div>
        <span>TextHalo setup</span>
        <button className="plain" onClick={() => void finish()}>Skip setup</button>
      </header>

      <div className="setup-wizard-progress" aria-label={`Step ${step + 1} of ${STEPS.length}`}>
        {STEPS.map((label, index) => <div key={label} className={index <= step ? "complete" : ""}>
          <span>{index + 1}</span>{label}
        </div>)}
      </div>

      <section className="setup-wizard-content" aria-live="polite">
        {step === 0 ? <>
          <div className="setup-wizard-kicker">FIRST, A QUICK PERMISSION</div>
          <h1>Let TextHalo hear<br />what you select.</h1>
          <p>TextHalo only reads a selection when you use your Speak shortcut. macOS Accessibility access lets it capture that selection; it does not monitor your typing.</p>
          {trusted ? <div className="setup-permission-state granted">✓ Accessibility access is on. You’re ready to continue.</div> : <>
            <div className="setup-permission-state">{settings.accessibility_prompted
              ? "In System Settings, enable TextHalo under Privacy & Security → Accessibility."
              : "macOS will ask for Accessibility access now. You can grant it in the system prompt or enable it in System Settings."}</div>
            {settings.accessibility_prompted ? <button className="plain" onClick={onOpenAccessibilitySettings}>Open Accessibility Settings…</button> : <button className="plain" onClick={onRequestAccessibility}>Show permission prompt again</button>}
          </>}
        </> : null}

        {step === 1 ? <>
          <div className="setup-wizard-kicker">YOUR STARTING VOICE</div>
          <h1>Pick a voice<br />that feels right.</h1>
          <p>TextHalo starts with Apple voices, which are ready to use on your Mac. You can browse local AI and hosted voices later in Voice settings.</p>
          <label className="setup-field"><span>Apple voice</span><select value={settings.voice ?? ""} onChange={(event) => void onSave({ voice: event.target.value || null })}>
            <option value="">System default</option>
            {speechVoices.map((voice) => <option key={voice.name} value={voice.name}>{voice.name} · {voice.locale.replace("_", "-")}</option>)}
          </select></label>
          <button className="plain" onClick={() => onPreviewVoice(settings.voice)}>▶ Preview this voice</button>
        </> : null}

        {step === 2 ? <>
          <div className="setup-wizard-kicker">A SHORTCUT FOR LISTENING</div>
          <h1>Speak from<br />any app.</h1>
          <p>Select text anywhere, then use this shortcut to hear it read aloud. Keep the default or record one that’s easier to remember.</p>
          <div className="setup-shortcut-display">{recordingShortcut ? "Press your new shortcut… (Esc to cancel)" : settings.shortcuts.speak.split("+").join("  +  ")}</div>
          <button className="plain" onClick={onRecordShortcut}>{recordingShortcut ? "Listening for keys…" : "Change shortcut…"}</button>
        </> : null}

        {step === 3 ? <>
          <div className="setup-wizard-kicker">YOU’RE ALL SET</div>
          <h1>A little more<br />room to listen.</h1>
          <p>Choose text in another app and press <strong>{settings.shortcuts.speak.split("+").join(" + ")}</strong>. You can change voices, shortcuts, and capture preferences any time in Settings.</p>
          {!trusted ? <div className="setup-permission-state">You can finish setup now, but grant Accessibility access before trying to read selections.</div> : null}
        </> : null}
      </section>

      <footer className="setup-wizard-footer">
        {step > 0 ? <button className="plain" onClick={() => setStep((current) => current - 1)}>Back</button> : <span />}
        {step < STEPS.length - 1
          ? <button className="primary-button" onClick={() => setStep((current) => current + 1)}>Continue</button>
          : <button className="primary-button" onClick={() => void finish()}>Start listening</button>}
      </footer>
    </main>
  );
}
