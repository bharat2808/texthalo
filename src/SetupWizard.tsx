import { useEffect, useRef, useState } from "react";
import appLogo from "../src-tauri/icons/icon.png";
import "./SetupWizard.css";

type WizardSettings = {
  onboarding_completed: boolean;
  launch_at_login: boolean;
  accessibility_prompted: boolean;
  engine: "apple" | "kokoro" | "chatterbox" | "fish";
  shortcuts: { speak: string; stop: string };
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
  recordingShortcut: boolean;
  kokoroReady: boolean;
  kokoroDownloadBytes: number;
  kokoroInstall: { phase: "downloading" | "done" | "error"; done: number; total: number; message: string | null } | null;
  onSave: (patch: Partial<WizardSettings>) => void | Promise<void>;
  onRequestAccessibility: () => void;
  onOpenAccessibilitySettings: () => void;
  onPreviewVoice: (voice: string | null) => void;
  onChooseEngine: (engine: "apple" | "kokoro") => void;
  onInstallKokoro: () => void;
  onRecordShortcut: () => void;
  onFinish: () => void | Promise<void>;
  onLoadPlans: () => Promise<SetupPlansResponse>;
  onOpenPlans: () => void;
};

const STEPS = ["Access", "Voice", "Shortcut", "Ready"];

function formatPlanPrice(price: SetupPlan["price"]): string {
  if (!price) return "Pricing unavailable";
  const formatter = new Intl.NumberFormat(undefined, { style: "currency", currency: price.currency });
  const digits = formatter.resolvedOptions().maximumFractionDigits ?? 2;
  const amount = formatter.format(price.unitAmount / (10 ** digits));
  if (!price.interval) return amount;
  const count = price.intervalCount ?? 1;
  return `${amount} / ${count > 1 ? `${count} ` : ""}${price.interval}${count > 1 ? "s" : ""}`;
}

export default function SetupWizard({
  settings,
  voices,
  trusted,
  recordingShortcut,
  kokoroReady,
  kokoroDownloadBytes,
  kokoroInstall,
  onSave,
  onRequestAccessibility,
  onOpenAccessibilitySettings,
  onPreviewVoice,
  onChooseEngine,
  onInstallKokoro,
  onRecordShortcut,
  onFinish,
  onLoadPlans,
  onOpenPlans,
}: SetupWizardProps) {
  const [step, setStep] = useState(0);
  const plansRequested = useRef(false);
  const [plans, setPlans] = useState<SetupPlan[] | null>(null);
  const [plansError, setPlansError] = useState(false);
  const [selectedEngine, setSelectedEngine] = useState<"apple" | "kokoro" | null>(
    settings.engine === "apple" || settings.engine === "kokoro" ? settings.engine : null,
  );
  const [startingKokoro, setStartingKokoro] = useState(false);
  const speechVoices = voices.filter((voice) => !voice.novelty);
  const kokoroInstalled = kokoroReady || kokoroInstall?.phase === "done";
  const kokoroInstalling = startingKokoro || kokoroInstall?.phase === "downloading";
  const kokoroProgress = kokoroInstall && kokoroInstall.total > 0
    ? Math.min(100, Math.round((kokoroInstall.done / kokoroInstall.total) * 100))
    : 0;
  const canContinue = step !== 1 || selectedEngine === "apple" || (selectedEngine === "kokoro" && kokoroInstalled);

  useEffect(() => {
    if (kokoroInstall) setStartingKokoro(false);
  }, [kokoroInstall]);

  useEffect(() => {
    if (step !== 3 || plansRequested.current) return;
    plansRequested.current = true;
    void onLoadPlans().then((result) => setPlans(result.plans)).catch(() => setPlansError(true));
  }, [onLoadPlans, step]);

  const finish = async () => {
    if (settings.engine === "kokoro" && !kokoroInstalled) await onSave({ engine: "apple" });
    await onSave({ onboarding_completed: true });
    await onFinish();
  };

  const chooseApple = () => {
    setSelectedEngine("apple");
    onChooseEngine("apple");
  };

  const chooseKokoro = () => {
    setSelectedEngine("kokoro");
    onChooseEngine("kokoro");
    if (!kokoroInstalled && !kokoroInstalling) {
      setStartingKokoro(true);
      onInstallKokoro();
    }
  };

  return (
    <main className="setup-wizard">
      <header className="setup-wizard-header">
        <img className="setup-wizard-mark" src={appLogo} alt="" />
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
          <h1>Start with a voice<br />you love.</h1>
          <p>Apple is ready now. Want more local AI voices? Set up Kokoro here, or come back to it later in Voice settings.</p>
          <div className="setup-engine-choice">
            <article className={selectedEngine === "apple" ? "setup-engine-card selected" : "setup-engine-card"}>
              <div><strong>Apple voices</strong><span>Ready immediately · no model download</span></div>
              <button className="plain" onClick={chooseApple}>{selectedEngine === "apple" ? "Selected" : "Use Apple"}</button>
              {selectedEngine === "apple" ? <>
                <label className="setup-field"><span>Choose an installed voice</span><select value={settings.voice ?? ""} onChange={(event) => void onSave({ voice: event.target.value || null })}>
                  <option value="">System default</option>
                  {speechVoices.map((voice) => <option key={voice.name} value={voice.name}>{voice.name} · {voice.locale.replace("_", "-")}</option>)}
                </select></label>
                <button className="plain" onClick={() => onPreviewVoice(settings.voice)}>▶ Preview Apple voice</button>
              </> : null}
            </article>
            <article className={selectedEngine === "kokoro" ? "setup-engine-card selected" : "setup-engine-card"}>
              <div><strong>Kokoro <span className="setup-recommended">RECOMMENDED</span></strong><span>28 English local AI voices · stays on your Mac</span></div>
              {kokoroInstalled ? <button className="plain" onClick={chooseKokoro}>{selectedEngine === "kokoro" ? "Selected" : "Use Kokoro"}</button> : <button className="plain" disabled={kokoroInstalling} onClick={chooseKokoro}>{kokoroInstalling ? "Downloading…" : kokoroInstall?.phase === "error" ? "Retry download" : `Download ${Math.round(kokoroDownloadBytes / 1_000_000)} MB`}</button>}
              {kokoroInstalling ? <div className="setup-download-status"><div className="progress"><div className="progress-bar" style={{ width: `${kokoroProgress}%` }} /></div><span>{kokoroProgress}% · {Math.round((kokoroInstall?.done ?? 0) / 1_000_000)} of {Math.round((kokoroInstall?.total || kokoroDownloadBytes) / 1_000_000)} MB</span></div> : null}
              {kokoroInstall?.phase === "error" ? <span className="setup-install-error">{kokoroInstall.message ?? "Download failed. Check your connection and try again."}</span> : null}
              {kokoroInstalled ? <span className="setup-kokoro-ready">✓ Downloaded and ready</span> : !kokoroInstalling && kokoroInstall?.phase !== "error" ? <span>One-time download · about {Math.round(kokoroDownloadBytes / 1_000_000)} MB</span> : null}
            </article>
          </div>
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
          <h1>Free to start.<br />More when you need it.</h1>
          <p>Choose text in another app and press <strong>{settings.shortcuts.speak.split("+").join(" + ")}</strong>. Local voices are free; hosted voices are optional.</p>
          {plans ? <div className="setup-plans" aria-label="TextHalo plans">
            {plans.map((plan) => <article className={`setup-plan-card${plan.id === "creator" ? " featured" : ""}`} key={plan.id}>
              <strong>{plan.name}</strong>
              <span className="setup-plan-price">{formatPlanPrice(plan.price)}</span>
              <span>{plan.description}</span>
              {plan.features.filter((feature) => feature !== "Everything in Free").slice(0, 3).map((feature) => <small key={feature}>{feature}</small>)}
            </article>)}
          </div> : <span className="setup-plan-loading">{plansError ? "Plan details aren’t available right now." : "Loading current plans…"}</span>}
          <button className="plain" onClick={onOpenPlans}>Compare plans ↗</button>
          {!trusted ? <div className="setup-permission-state">You can finish setup now, but grant Accessibility access before trying to read selections.</div> : null}
          <label className="toggle-row setup-launch-toggle">
            <input
              type="checkbox"
              checked={settings.launch_at_login}
              onChange={(event) => void onSave({ launch_at_login: event.target.checked })}
            />
            <span>Start TextHalo automatically when I log in</span>
          </label>
        </> : null}
      </section>

      <footer className="setup-wizard-footer">
        {step > 0 ? <button className="plain" onClick={() => setStep((current) => current - 1)}>Back</button> : <span />}
        {step < STEPS.length - 1
          ? <button className="primary-button" disabled={!canContinue} onClick={() => setStep((current) => current + 1)}>Continue</button>
          : <button className="primary-button" onClick={() => void finish()}>Start listening</button>}
      </footer>
    </main>
  );
}
