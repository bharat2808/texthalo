import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
export type AssistantPreferences = { language: string; instruction: string; dictation_output: string; context: string };
export const defaultAssistantPreferences: AssistantPreferences = { language: "en-US", instruction: "", dictation_output: "insert", context: "selection" };
type Availability = { model?: string; microphone?: number; recognition?: number; accessibility?: boolean; diagnostic?: string };
export default function AssistantSettings({ value, onChange }: { value: AssistantPreferences; onChange: (value: AssistantPreferences) => void }) {
  const [availability, setAvailability] = useState<Availability>({});
  const [error, setError] = useState("");
  const [instruction, setInstruction] = useState(value.instruction);
  useEffect(() => { setInstruction(value.instruction); }, [value.instruction]);
  async function check(command: string) {
    setError("");
    try { await invoke("assistant_control", { command }); } catch (cause) { setError(String(cause)); }
  }
  useEffect(() => {
    const off = listen<Availability>("texthalo:assistant-availability", event => setAvailability(event.payload));
    void off.then(() => check("status"));
    return () => { void off.then(unlisten => unlisten()); };
  }, []);
  return <div className="assistant-settings">
    <p className="card-note">Hold Transcribe to dictate; release to insert. Hold Assistant to speak a request; release to generate or narrate. Escape cancels. New shortcuts are unassigned until you choose them above.</p>
    <div className="field"><label className="field-label" htmlFor="recognition-language">Transcription language</label>
      <select id="recognition-language" value={value.language} onChange={event => onChange({ ...value, language: event.target.value })}>
        {[["en-US","English (US)"],["en-CA","English (Canada)"],["en-GB","English (UK)"],["fr-FR","French"],["de-DE","German"],["es-ES","Spanish"],["it-IT","Italian"],["pt-BR","Portuguese"],["ja-JP","Japanese"],["zh-CN","Chinese"],["hi-IN","Hindi"]].map(([code,label]) => <option key={code} value={code}>{label}</option>)}
      </select><p className="card-note">On-device availability depends on installed macOS language assets. Audio never falls back to a cloud recognizer.</p></div>
    <div className="field"><label className="field-label" htmlFor="dictation-output">Transcription result</label><select id="dictation-output" value={value.dictation_output} onChange={event => onChange({ ...value, dictation_output: event.target.value })}>
      <option value="insert">Insert into the unchanged original field</option><option value="copy">Copy to clipboard</option></select></div>
    <div className="field"><label className="field-label" htmlFor="assistant-context">Assistant context</label><select id="assistant-context" value={value.context} onChange={event => onChange({ ...value, context: event.target.value })}>
      <option value="selection">Selected text at shortcut press</option><option value="none">Only my spoken request</option></select><p className="card-note">Unsupported selections can be added through screen-text capture in the assistant panel. Screen capture is always explicit.</p></div>
    <div className="field"><label className="field-label" htmlFor="assistant-instruction">Writing preferences</label><textarea id="assistant-instruction" rows={3} maxLength={2000} value={instruction} onChange={event => setInstruction(event.target.value)} onBlur={() => { if (instruction !== value.instruction) onChange({ ...value, instruction }); }} placeholder="Keep responses concise. Use a warm, conversational tone." /></div>
    <div className="card-note" role="status">Apple model: {availability.model ?? "Checking…"}<br />Microphone: {availability.microphone === 3 ? "Allowed" : "Permission required"} · Speech recognition: {availability.recognition === 3 ? "Allowed" : "Permission required"}<br />Accessibility: {availability.accessibility ? "Allowed" : "Required for selection and insertion"}</div>
    <div className="assistant-actions"><button onClick={() => void check("permissions")}>Enable microphone and speech</button><button onClick={() => void check("status")}>Refresh availability</button><button onClick={() => void check("diagnose")}>Test local transcription</button><button onClick={() => void check("open")}>Open assistant</button></div>
    {availability.diagnostic && <p role="status">{availability.diagnostic}</p>}
    {error && <p role="alert">{error}</p>}
  </div>;
}
