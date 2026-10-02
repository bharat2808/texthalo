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
      {name:"Microphone",key:"microphone",status:a.microphone,description:"Records while you hold Transcription or Assistant."},
      {name:"Speech Recognition",key:"speech",status:a.recognition === 1 ? 2 : a.recognition === 2 ? 1 : a.recognition,description:"Converts your voice into text on this Mac."},
      {name:"Screen Recording",key:"screen",status:a.screen === undefined ? undefined : a.screen ? 3 : 0,description:"Reads visible text from the original window when Assistant screen context is enabled. Uses local OCR."},
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
      <p className="card-note">Applied with each spoken request. Written output inserts or replaces directly, with Undo. Requests to read aloud use your selected voice.</p>
    </div>
    <div className="field"><label className="field-label" htmlFor="assistant-context">Assistant context</label>
      <select id="assistant-context" value={value.context} onChange={event => onChange({ ...value, instruction: draft.current, context: event.target.value })}>
        <option value="selection">Selected text</option><option value="screen">Selected text and original window</option><option value="none">Only my spoken request</option>
      </select><p className="card-note">Window context extracts visible text locally when you invoke Assistant. It does not interpret images or diagrams.</p>
    </div>
    <p className="card-note" role="status">Apple model: {availability.model ?? "Checking…"}</p>
    {error && <p role="alert">{error}</p>}
  </div>;
}
export function TranscriptionSettings({value,onChange}: Props) {
  const {availability,error,command} = useAvailability();
  return <div className="assistant-settings">
    <div className="field"><label className="field-label" htmlFor="recognition-language">Transcription language</label>
      <select id="recognition-language" value={value.language} onChange={event => onChange({...value,language:event.target.value})}>
        {[["en-US","English (US)"],["en-CA","English (Canada)"],["en-GB","English (UK)"],["fr-FR","French"],["de-DE","German"],["es-ES","Spanish"],["it-IT","Italian"],["pt-BR","Portuguese"],["ja-JP","Japanese"],["zh-CN","Chinese"],["hi-IN","Hindi"]].map(([code,label]) => <option key={code} value={code}>{label}</option>)}
      </select><p className="card-note">Also used for spoken Assistant requests. Requires an available on-device recognizer for the chosen language.</p>
    </div>
    <div className="field"><label className="field-label" htmlFor="dictation-output">Transcription result</label>
      <select id="dictation-output" value={value.dictation_output} onChange={event => onChange({...value,dictation_output:event.target.value})}>
        <option value="insert">Insert or replace directly, with Undo</option><option value="copy">Copy to clipboard</option>
      </select>
    </div>
    <button className="plain" onClick={() => void command("diagnose")}>Test local transcription</button>
    <p className="card-note">Uses generated test audio; does not record your microphone.</p>
    {availability.diagnostic && <p role="status">{availability.diagnostic}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}
