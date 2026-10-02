import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
type Entry = { id:string; createdAt:number; text:string; source:string };
export default function TranscriptionHistory() {
  const [entries,setEntries] = useState<Entry[]>([]);
  const [error,setError] = useState("");
  const [loaded,setLoaded] = useState(false);
  const [copied,setCopied] = useState<string|null>(null);
  async function refresh(command = "list", id?:string) {
    setError("");
    try { setEntries(await invoke<Entry[]>("transcription_history",{command,id})); setLoaded(true); }
    catch(cause) { setError(String(cause)); }
  }
  useEffect(()=>{
    const off = listen<{error?:string}>("texthalo:transcription-history",({payload})=>{
      if(payload.error) setError(payload.error); else void refresh();
    });
    void off.then(()=>refresh());
    return ()=>{void off.then(fn=>fn());};
  },[]);
  return <div>
    <p className="card-note">Your latest 50 transcripts, stored on this Mac. Microphone audio and screen context are not saved here.</p>
    {error && <p role="alert">{error}</p>}
    {!loaded && !error && <p>Loading transcripts…</p>}
    {loaded && entries.length === 0 && <p className="card-note">No transcriptions yet. Hold your Transcription shortcut and release to finish.</p>}
    {entries.map(entry=><article className="permission-row" key={entry.id}>
      <div className="card-note">{new Date(entry.createdAt).toLocaleString()}{entry.source ? ` · ${entry.source}` : ""}</div>
      <p className="transcript-text">{entry.text}</p>
      <div className="inline"><button className="plain" onClick={()=>{void navigator.clipboard.writeText(entry.text).then(()=>setCopied(entry.id)).catch(cause=>setError(String(cause)));}}>{copied===entry.id?"Copied":"Copy transcript"}</button>
        <button className="plain" aria-label="Delete transcript" onClick={()=>void refresh("delete",entry.id)}>Delete</button></div>
    </article>)}
    {entries.length>0 && <div className="inline"><button className="plain" onClick={()=>void refresh("clear")}>Clear transcription history</button></div>}
  </div>;
}
