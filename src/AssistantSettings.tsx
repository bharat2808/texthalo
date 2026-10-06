import { t, message, currentLanguage } from "./i18n";
import SelectionRow from "./SelectionRow";
import { localAssistantReason } from "./assistantAvailability";
import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { groupRecognitionLanguages, type RecognitionLanguage } from "./recognitionLanguages";
export type AssistantPreferences = { language: string; instruction: string; dictation_output: string; transcription_engine?: "apple" | "whisper"; whisper_model?: string; provider?: "local" | "cloud" | "automatic"; cloud_consent_version?: number | null };
export const defaultAssistantPreferences: AssistantPreferences = { language: "en-US", transcription_engine: "whisper", whisper_model: "small", instruction: "", dictation_output: "insert", provider: "local", cloud_consent_version: null };
export type Availability = { model?: string; microphone?: number; recognition?: number; accessibility?: boolean; screen?: boolean; imageInput?: boolean; cloudScreenInput?: boolean; holdError?: string; cloudConfigured?: boolean; cloudSignedIn?: boolean; provider?: string; effectiveProvider?: string; whisperReady?: boolean; whisperLanguages?: RecognitionLanguage[]; speechLanguages?: RecognitionLanguage[] };
type SettingsProps = { value: AssistantPreferences; onChange: (value: AssistantPreferences) => void };
type Props = SettingsProps & { onSignIn: () => void; onCancelSignIn: () => void; signingIn: boolean; warningIcon: ReactNode; accountMark: string; signInError: string };
export function useAvailability() {
  const [availability, setAvailability] = useState<Availability>({});
  const [error, setError] = useState("");
  async function command(command: string) {
    setError("");
    try { await invoke("assistant_control", { command }); } catch (cause) { setError(String(cause)); }
  }
  useEffect(() => {
    let alive = true;
    let received = false;
    const off = listen<Availability>("texthalo:assistant-availability", event => { received = true; if (alive) setAvailability(event.payload); });
    void off.then(async () => {
      const state = await invoke<{availability:Availability}>("assistant_state");
      if (alive && !received) setAvailability(state.availability ?? {});
      if (alive) await command("status");
    }).catch(cause => { if (alive) setError(String(cause)); });
    const poll = window.setInterval(() => { void command("status"); }, 3000);
    const refresh = () => { void command("status"); };
    window.addEventListener("focus", refresh);
    return () => { alive = false; clearInterval(poll); window.removeEventListener("focus", refresh); void off.then(unlisten => unlisten()); };
  }, []);
  return { availability, error, command };
}
export function PermissionsSettings() {
  const {availability:a,error,command} = useAvailability();
  const label = (status?: number) => status === 3 ? t("Allowed") : status === 2 ? t("Denied") : status === 1 ? t("Restricted") : status === 0 ? t("Not requested") : t("Checking…");
  return <div>
    {([
      {name:t("Microphone"),key:"microphone",status:a.microphone,description:t("Records while you hold Dictate or Assistant.")},
      {name:t("Speech Recognition"),key:"speech",status:a.recognition === 1 ? 2 : a.recognition === 2 ? 1 : a.recognition,description:t("Required only for macOS speech recognition.")},
      {name:t("Screen Recording"),key:"screen",status:a.screen === undefined ? undefined : a.screen ? 3 : 0,description:a.imageInput === false && a.cloudScreenInput !== true ? t("Screen context is unavailable on this Mac.") : t("Lets Assistant capture your current window when you make a request.")},
    ]).map(p => <section className="permission-row" key={p.key}>
      <strong>{p.name}</strong><div className="card-note">{p.description}</div><p>{label(p.status)}</p>
      <div className="inline">
        {p.status !== 3 && <button className="plain accent-action" disabled={p.status === undefined || p.status === 1} onClick={() => void command(`permission_${p.key}`)}>{t("Enable")} {p.name}</button>}
        <button className="plain accent-action" onClick={() => void command(`settings_${p.key}`)}>{t("Open")} {p.name}  {t("Settings…")}</button>
      </div>
    </section>)}
    {error && <p role="alert">{message(error)}</p>}
  </div>;
}
export default function AssistantSettings({ value, onChange, onSignIn, onCancelSignIn, signingIn, warningIcon, accountMark, signInError }: Props) {
  const {availability,error} = useAvailability();
  const [instruction, setInstruction] = useState(value.instruction);
  const provider = value.provider ?? "local";
  const localUnavailable = availability.model !== undefined && availability.model !== "available";
  const usesCloud = provider === "cloud" || (provider === "automatic" && localUnavailable);
  // Preserve the draft when another control saves in the same event turn.
  const draft = useRef(value.instruction);
  useEffect(() => { draft.current = value.instruction; setInstruction(value.instruction); }, [value.instruction]);
  return <div className="assistant-settings">
    <div className="field"><span className="field-label" id="assistant-provider-label">{t("Assistant model")}</span>
      <div className="row-stack" role="group" aria-labelledby="assistant-provider-label">
        {([
          { id: "local", title: t("On this Mac"), subtitle: localAssistantReason(availability.model) },
          { id: "cloud", title: "TextHalo Cloud", subtitle: t("Hosted · uses credits") },
          { id: "automatic", title: t("Automatic"), subtitle: availability.model === undefined ? t("Checking which provider is available…") : localUnavailable ? t("Will use TextHalo Cloud · uses credits") : t("Will use Apple Intelligence · no credits") },
        ] as const).map(provider => <SelectionRow
          key={provider.id}
          selected={(value.provider ?? "local") === provider.id}
          title={provider.title}
          subtitle={provider.subtitle}
          badge={provider.id === "local" && localUnavailable ? t("Unavailable") : (value.provider ?? "local") === provider.id ? t("Selected") : undefined}
          disabled={provider.id === "local" && localUnavailable}
          onSelect={() => onChange({ ...value, instruction: draft.current, provider: provider.id })}
        />)}
      </div>
      {provider === "local" && localUnavailable && <>
        <div className="note-line warning">{warningIcon}<span>{t("On-device Assistant is unavailable on this Mac.")}</span></div>
        {availability.cloudConfigured && <div className="inline"><button className="plain accent-action" onClick={() => onChange({ ...value, instruction: draft.current, provider: "cloud" })}>{t("Use Cloud Assistant")}</button></div>}
      </>}
      <p className="card-note">{t("Hold the same Assistant shortcut to speak. Cloud fulfills your request using your TextHalo credit balance. Automatic uses Cloud when Apple Intelligence is unavailable at the start of a request.")}</p>
    </div>
    <div className="field">
      {provider !== "local" && <label className="checkbox-row"><input type="checkbox" checked={value.cloud_consent_version === 1} onChange={event => onChange({ ...value, instruction: draft.current, cloud_consent_version: event.target.checked ? 1 : null })} />{t("Enable cloud processing for Assistant")}</label>}
      <p className="card-note"><strong>{t("Privacy:")}</strong>  {t("Your spoken request is transcribed on this Mac. TextHalo captures your current window only while you use Assistant. Your latest 50 runs—including requests, selected text, screenshots, prompts, and answers—are saved locally in Assistant history.")} {provider !== "local" && <>{t("Cloud processing sends your transcript, instructions, selected text, screen text, and previous answer when used to TextHalo and its model provider. Screenshots and microphone audio are not uploaded for Assistant generation. TextHalo’s server-side Assistant records do not store prompts or answers; account, billing, and usage records are retained. Providers may retain request content under their own policies.")} </>}{t("A hosted voice separately sends the words being spoken to TextHalo and Fish Audio.")}</p>
      {provider !== "local" && <p className="card-note">{t("Cloud answers use credits; a hosted voice uses additional credits. Canceling may still use credits for generation already performed.")}</p>}
      {provider !== "local" && !availability.cloudConfigured && <p className="card-note">{t("Cloud Assistant is not configured in this build.")}</p>}
      {provider !== "local" && availability.cloudConfigured && !availability.cloudSignedIn && <>
        <div className="note-line warning">{warningIcon}<span>{t("Sign in to use Cloud Assistant.")}</span></div>
        <div className="account-brand">
          <img className="account-brand-mark" src={accountMark} alt="" />
          <div><strong>{t("TextHalo")}</strong><div className="card-note">{t("Your account for Cloud Assistant")}</div></div>
          <div className="account-brand-actions">
            <button className="plain accent-action" disabled={signingIn} onClick={onSignIn}>{signingIn ? t("Waiting for browser sign-in…") : t("Sign in")}</button>
            {signingIn && <button className="text" onClick={onCancelSignIn}>{t("Cancel")}</button>}
          </div>
        </div>
        {signInError && <p className="note-line error" role="alert">{signInError}</p>}
      </>}
    </div>
    <div className="field"><label className="field-label" htmlFor="assistant-instruction">{t("Assistant instructions")}</label>
      <textarea id="assistant-instruction" rows={7} maxLength={2000} value={instruction} onChange={event => { draft.current = event.target.value; setInstruction(event.target.value); }} onBlur={() => { if (draft.current !== value.instruction) onChange({ ...value, instruction: draft.current }); }} placeholder={t("Use a warm tone. Keep writing concise. When I ask for a voiceover, write a script ready to read aloud.")} />
      <p className="card-note">{t("Your preferences for every request. You can undo text edits.")}</p>
    </div>
    {(usesCloud ? availability.cloudScreenInput : availability.imageInput) === false && <p className="card-note">{t("Screen context is unavailable for the selected provider on this Mac. Assistant can still use your request and selected text.")}</p>}
    {error && <p role="alert">{message(error)}</p>}
  </div>;
}
export function TranscriptionSettings({value,onChange}: SettingsProps) {
  const {availability,error} = useAvailability();
  const whisper = value.transcription_engine === "whisper";
  const languages = whisper ? availability.whisperLanguages : availability.speechLanguages;
  const selectedLanguage = whisper ? value.language.split(/[-_]/)[0] : value.language;
  const groupedLanguages = groupRecognitionLanguages(languages ?? [], currentLanguage());
  return <div className="assistant-settings">
    <div className="field"><label className="field-label" htmlFor="recognition-language">{t("Language")}</label>
      <select id="recognition-language" value={selectedLanguage} onChange={event => onChange({...value,language:event.target.value})}>
        {whisper && <option value="auto">{t("Automatic detection")}</option>}
        {selectedLanguage !== "auto" && !languages?.some(language => language.code === selectedLanguage) && <option value={selectedLanguage}>{selectedLanguage}</option>}
        {groupedLanguages.map(group => whisper ? <option key={group.name} value={group.languages[0].code}>{group.name}</option> : <optgroup key={group.name} label={group.name}>
          {group.languages.map(language => <option key={language.code} value={language.code}>{new Intl.DisplayNames([currentLanguage()], { type: "language" }).of(language.code.replace(/_/g, "-")) ?? language.name}</option>)}
        </optgroup>)}
      </select>
      <p className="card-note">{t("Also used by Assistant. Speech is transcribed on this Mac using")} {whisper ? t("Whisper. Choose a language for more consistent results; automatic detection can vary for mixed-language speech.") : t("macOS speech recognition.")}</p>
    </div>
    <div className="field"><label className="field-label" htmlFor="dictation-output">{t("When I finish speaking")}</label>
      <select id="dictation-output" value={value.dictation_output} onChange={event => onChange({...value,dictation_output:event.target.value})}>
        <option value="insert">{t("Insert into the current app")}</option><option value="copy">{t("Copy to clipboard")}</option>
      </select>
    </div>
    {error && <p role="alert">{message(error)}</p>}
  </div>;
}

type FeaturePermission = {
  name: string;
  description: string;
  key: "microphone" | "speech" | "accessibility" | "screen";
  status: number | undefined;
  required: boolean;
};

export function TaskReadiness({ mode, needsAccessibility = true, accessibilityPrompted, onAccessibility, preferences = defaultAssistantPreferences }: {
  mode: "dictate" | "assistant";
  needsAccessibility?: boolean;
  accessibilityPrompted: boolean;
  onAccessibility: () => Promise<void>;
  preferences?: AssistantPreferences;
}) {
  const { availability: a, error } = useAvailability();
  const [pending, setPending] = useState<string | null>(null);
  const [requestError, setRequestError] = useState("");
  const [screenRequested, setScreenRequested] = useState(false);
  const requesting = useRef(false);
  const permissions: FeaturePermission[] = [
    { name: t("Microphone"), description: mode === "dictate" ? t("Records your speech for dictation.") : t("Records your spoken requests."), key: "microphone", status: a.microphone, required: true },
    // Speech Recognition's denied/restricted enum values are reversed from Microphone.
    { name: t("Speech Recognition"), description: t("Converts your speech to text using macOS."), key: "speech", status: a.recognition === 1 ? 2 : a.recognition === 2 ? 1 : a.recognition, required: preferences.transcription_engine !== "whisper" },
    { name: t("Accessibility"), description: mode === "dictate" ? t("Inserts dictated text into the current app.") : t("Reads selected text and inserts responses into other apps."), key: "accessibility", status: a.accessibility === undefined ? undefined : a.accessibility ? 3 : 0, required: needsAccessibility },
    ...(mode === "assistant" ? [{ name: t("Screen Recording"), description: t("Captures the current window as context for your request."), key: "screen" as const, status: a.screen === undefined ? undefined : a.screen ? 3 : 0, required: (preferences.provider === "cloud" || (preferences.provider === "automatic" && a.model !== "available") ? a.cloudScreenInput ?? a.imageInput : a.imageInput) !== false }] : []),
  ];
  const missing = permissions.filter(permission => permission.required && permission.status !== 3);
  const checking = missing.some(permission => permission.status === undefined);

  // Permission IPC acknowledges the send; the system dialog resolves later.
  // Availability events (also refreshed by polling) tell us when it has resolved.
  useEffect(() => {
    const status = pending === "microphone" ? a.microphone : pending === "speech" ? a.recognition : undefined;
    if (error || (status !== undefined && status !== 0)) {
      requesting.current = false;
      setPending(null);
    }
  }, [pending, a.microphone, a.recognition, error]);

  async function request(permission: FeaturePermission) {
    if (requesting.current || !permission.required || permission.status === 3 || permission.status === undefined || permission.status === 1) return;
    requesting.current = true;
    setPending(permission.key); setRequestError("");
    let waitForResult = permission.status === 0 && (permission.key === "microphone" || permission.key === "speech");
    try {
      if (permission.key === "accessibility") await onAccessibility();
      else {
        const openSettings = permission.status === 2 || (permission.key === "screen" && screenRequested);
        await invoke("assistant_control", { command: `${openSettings ? "settings" : "permission"}_${permission.key}` });
        if (permission.key === "screen") setScreenRequested(true);
      }
    } catch (cause) { waitForResult = false; setRequestError(String(cause)); }
    finally { if (!waitForResult) { requesting.current = false; setPending(null); } }
  }

  return <section className="feature-permissions" aria-label={t("{0} access", [mode === "assistant" ? t("Assistant") : t("Dictate")])}>
    <div className="permission-summary"><strong>{t("Access")}</strong><span role="status">{checking ? t("Checking…") : missing.length ? t("{0} needed", [missing.length]) : t("Ready")}</span></div>
    <ul className="permission-checklist">
      {permissions.map(permission => {
        const granted = permission.status === 3;
        const restricted = permission.status === 1;
        const settings = permission.status === 2 || (permission.key === "accessibility" && accessibilityPrompted) || (permission.key === "screen" && screenRequested);
        const label = !permission.required ? permission.key === "speech" ? t("Only for macOS speech recognition") : permission.key === "screen" ? t("Unavailable for the selected provider on this Mac") : t("Only for inserting text") : permission.status === undefined ? t("Checking…") : granted ? t("Allowed") : restricted ? t("Restricted by macOS") : t("Not allowed");
        return <li key={permission.key}><div><span>{permission.name}</span><small>{label}</small>{permission.required && !granted ? <small>{permission.description}</small> : null}</div>
          {permission.required && !granted && !restricted ? <button className="plain accent-action" disabled={pending !== null || permission.status === undefined} onClick={() => void request(permission)}>{pending === permission.key ? (permission.key === "microphone" || permission.key === "speech") && permission.status === 0 ? t("Waiting for macOS…") : t("Opening…") : settings ? t("Open {0} Settings", [permission.name]) : t("Enable {0}", [permission.name])}</button> : null}
        </li>;
      })}
    </ul>
    {requestError || error ? <p className="shortcut-error" role="alert">{requestError || error}</p> : null}
  </section>;
}
