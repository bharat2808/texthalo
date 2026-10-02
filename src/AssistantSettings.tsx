import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
export type AssistantPreferences = { language: string; instruction: string; dictation_output: string; context: string };
export const defaultAssistantPreferences: AssistantPreferences = { language: "en-US", instruction: "", dictation_output: "insert", context: "selection" };
type Availability = { model?: string; microphone?: number; recognition?: number; accessibility?: boolean; screen?: boolean; diagnostic?: string };
type Props = { value: AssistantPreferences; onChange: (value: AssistantPreferences) => void };
function useAvailability() {
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
    return () => { alive = false; clearInterval(poll); void off.then(unlisten => unlisten()); };
  }, []);
  return { availability, error, command };
}
export function PermissionsSettings() {
  const {availability:a,error,command} = useAvailability();
  const label = (status?: number) => status === 3 ? "Allowed" : status === 2 ? "Denied" : status === 1 ? "Restricted" : status === 0 ? "Not requested" : "Checking…";
  return <div>
    {([
      {name:"Microphone",key:"microphone",status:a.microphone,description:"Records while you hold Dictate or Assistant."},
      {name:"Speech Recognition",key:"speech",status:a.recognition === 1 ? 2 : a.recognition === 2 ? 1 : a.recognition,description:"Converts your voice into text on this Mac."},
      {name:"Screen Recording",key:"screen",status:a.screen === undefined ? undefined : a.screen ? 3 : 0,description:"Lets Assistant use text from your screen when you choose window context."},
    ]).map(p => <section className="permission-row" key={p.key}>
      <strong>{p.name}</strong><div className="card-note">{p.description}</div><p>{label(p.status)}</p>
      <div className="inline">
        {p.status !== 3 && <button className="plain" disabled={p.status === undefined || p.status === 1} onClick={() => void command(`permission_${p.key}`)}>Enable {p.name}</button>}
        <button className="plain" onClick={() => void command(`settings_${p.key}`)}>Open {p.name} Settings…</button>
      </div>
    </section>)}
    {error && <p role="alert">{error}</p>}
  </div>;
}
export default function AssistantSettings({ value, onChange }: Props) {
  const {availability,error} = useAvailability();
  const [instruction, setInstruction] = useState(value.instruction);
  // Preserve the draft when another control saves in the same event turn.
  const draft = useRef(value.instruction);
  useEffect(() => { draft.current = value.instruction; setInstruction(value.instruction); }, [value.instruction]);
  return <div className="assistant-settings">
    <div className="field"><label className="field-label" htmlFor="assistant-instruction">Assistant instructions</label>
      <textarea id="assistant-instruction" rows={7} maxLength={2000} value={instruction} onChange={event => { draft.current = event.target.value; setInstruction(event.target.value); }} onBlur={() => { if (draft.current !== value.instruction) onChange({ ...value, instruction: draft.current }); }} placeholder="Use a warm tone. Keep writing concise. When I ask for a voiceover, write a script ready to read aloud." />
      <p className="card-note">Your preferences for every request. You can undo text edits.</p>
    </div>
    <div className="field"><label className="field-label" htmlFor="assistant-context">Assistant context</label>
      <select id="assistant-context" value={value.context} onChange={event => onChange({ ...value, instruction: draft.current, context: event.target.value })}>
        <option value="selection">Selected text</option><option value="screen">Selected text and original window</option><option value="none">Only my spoken request</option>
      </select><p className="card-note">Window context reads visible text on your Mac, not images.</p>
    </div>
    <p className="card-note" role="status">Apple Intelligence: {availability.model ?? "Checking…"}</p>
    {error && <p role="alert">{error}</p>}
  </div>;
}
export function TranscriptionSettings({value,onChange}: Props) {
  const {availability,error,command} = useAvailability();
  return <div className="assistant-settings">
    <div className="field"><label className="field-label" htmlFor="recognition-language">Language</label>
      <select id="recognition-language" value={value.language} onChange={event => onChange({...value,language:event.target.value})}>
        {[["en-US","English (US)"],["en-CA","English (Canada)"],["en-GB","English (UK)"],["fr-FR","French"],["de-DE","German"],["es-ES","Spanish"],["it-IT","Italian"],["pt-BR","Portuguese"],["ja-JP","Japanese"],["zh-CN","Chinese"],["hi-IN","Hindi"]].map(([code,label]) => <option key={code} value={code}>{label}</option>)}
      </select><p className="card-note">Also used by Assistant. Availability depends on your Mac.</p>
    </div>
    <div className="field"><label className="field-label" htmlFor="dictation-output">When I finish speaking</label>
      <select id="dictation-output" value={value.dictation_output} onChange={event => onChange({...value,dictation_output:event.target.value})}>
        <option value="insert">Insert into the current app</option><option value="copy">Copy to clipboard</option>
      </select>
    </div>
    <button className="plain" onClick={() => void command("diagnose")}>Check dictation</button>
    <p className="card-note">Checks speech recognition without recording you.</p>
    {availability.diagnostic && <p role="status">{availability.diagnostic}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}

export function TaskReadiness({ mode, needsScreen = false, onPermissions }: {
  mode: "dictate" | "assistant" | "overview";
  needsScreen?: boolean;
  onPermissions: () => void;
}) {
  const { availability, error } = useAvailability();
  const checking = availability.microphone === undefined || availability.recognition === undefined;
  const needsAccess = !checking && (availability.microphone !== 3 || availability.recognition !== 3 || availability.accessibility === false || (needsScreen && availability.screen !== true));
  const modelUnavailable = mode !== "dictate" && availability.model !== undefined && availability.model !== "available";
  return <div className="readiness" role="status">
    {error ? <p>{error}</p> : checking ? <p>Checking voice access…</p> : needsAccess ? <div className="home-notice"><span>Enable access to use {mode === "dictate" ? "dictation" : mode === "assistant" ? "Assistant" : "dictation and Assistant"}.</span><button className="plain" onClick={onPermissions}>Enable access</button></div> : mode !== "overview" ? <p>Voice access is ready.</p> : null}
    {modelUnavailable ? <div>Assistant needs Apple Intelligence enabled on a supported Mac.<details><summary>Details</summary>{availability.model}</details></div> : null}
  </div>;
}
