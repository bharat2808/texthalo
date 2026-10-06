import { message, t } from "./i18n";

export type KokoroLanguageInstallEvent = { phase: "runtime" | "voices" | "done" | "error"; message: string };
type Voice = { language: string; requires_espeak?: boolean; requires_cjk?: boolean; unavailable: string | null };

export function additionalKokoroVoices(voices: readonly Voice[]) {
  return voices.filter(voice => voice.requires_espeak);
}

export default function KokoroLanguageSetup({ voices, event, busy, disabled, onDownload }: {
  voices: readonly Voice[];
  event: KokoroLanguageInstallEvent | null;
  busy: boolean;
  disabled: boolean;
  onDownload: () => void;
}) {
  const additional = additionalKokoroVoices(voices);
  const missing = additional.filter(voice => voice.unavailable !== null);
  const languages = [...new Set(additional.map(voice => voice.language))];
  return <>
    {missing.length > 0 || busy || event?.phase === "error" ? <div className="field kokoro-language-setup">
      <span className="field-label">{t("Additional languages")}</span>
      <span className="field-hint">{languages.map(language => t(language)).join(" · ")}</span>
      <button className="plain accent-action" disabled={disabled || busy} onClick={onDownload}>
        {busy ? event?.phase === "voices" ? t("Downloading voice files…") : t("Installing language support…") : event?.phase === "error" ? t("Retry voice setup") : t("Download {0} voices", [missing.length])}
      </button>
      <p className="card-note">{t("Installs language support and voice files together. Existing model files are reused.")}</p>
    </div> : null}
    {event ? <p className="card-note" role={event.phase === "error" ? "alert" : "status"}>{message(event.message)}</p> : null}
  </>;
}


export function KokoroCjkSetup({ voices, busy, disabled, error, onDownload }: {
  voices: readonly Voice[]; busy: boolean; disabled: boolean; error: string | null; onDownload: () => void;
}) {
  const missing = voices.filter(voice => voice.requires_cjk && voice.unavailable !== null);
  if (!missing.length) return null;
  return <div className="field kokoro-language-setup kokoro-cjk-setup">
    <span className="field-label">{t("Japanese and Mandarin Chinese")}</span>
    <span className="field-hint">{t("Pronunciation support is built in. Download the voice files to start listening.")}</span>
    <button className="plain accent-action" disabled={busy || disabled} onClick={onDownload}>
      {busy ? t("Downloading voice files…") : error ? t("Retry voice setup") : t("Download {0} voices", [missing.length])}
    </button>
    {error ? <p className="card-note" role="alert">{message(error)}</p> : null}
  </div>;
}
