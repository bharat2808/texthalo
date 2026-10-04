type Recovery = { text: string; canUndo?: boolean; copy: () => void; undo: () => void };
type Props = {
  assistant: boolean;
  message: string;
  actionError: string | null;
  busy: boolean;
  actionLabel: string;
  dismiss: () => void;
  recover: () => void;
  useLocal?: () => void;
  textRecovery?: Recovery;
};

export default function SpeechErrorPanel(props: Props) {
  const { assistant, message, actionError, busy, actionLabel, dismiss, recover, useLocal, textRecovery } = props;
  return <div className={`overlay-root speech-overlay error error-panel${textRecovery ? " with-recovery" : ""}`}>
    <div className="error-heading"><span className="activity" aria-hidden="true">!</span><strong>{assistant ? "Unable to answer" : "Unable to speak"}</strong>
      <button className="dismiss" disabled={busy} aria-label={assistant ? "Dismiss Assistant error" : "Dismiss speech error"} title="Dismiss" onClick={dismiss}>×</button>
    </div>
    <p className="error-reason" role="alert">{message}</p>
    {textRecovery && <div className="speech-transcript recovery-text" aria-label="Recoverable Assistant text" tabIndex={0}>{textRecovery.text}</div>}
    {actionError && <p className="recovery-error" role="alert">{actionError}</p>}
    <div className="recovery-actions">
      <button disabled={busy} onClick={recover}>{actionLabel}</button>
      {useLocal && <button className="secondary" disabled={busy} onClick={useLocal}>Use local voice</button>}
      {textRecovery && <button className="secondary" disabled={busy} onClick={textRecovery.copy}>Copy</button>}
      {textRecovery?.canUndo && <button className="secondary" disabled={busy} onClick={textRecovery.undo}>Undo</button>}
    </div>
  </div>;
}
