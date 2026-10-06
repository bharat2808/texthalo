import { t, message } from "./i18n";

export type KokoroModelOption = {
  id: string;
  label: string;
  description: string;
  bytes: number;
  installed: boolean;
  removable?: boolean;
};

export default function KokoroModelPicker({ models, selected, disabled, onSelect, onDownload, needsDownload = false, downloadLabel = t("Download selected model"), onDelete, deleting = false, deleteDisabled = false, engine = "Kokoro" }: {
  engine?: "Kokoro" | "Chatterbox";
  models: KokoroModelOption[];
  selected: string;
  disabled: boolean;
  onSelect: (id: string) => void;
  onDownload?: () => void;
  needsDownload?: boolean;
  downloadLabel?: string;
  onDelete?: () => void;
  deleting?: boolean;
  deleteDisabled?: boolean;
}) {
  const current = models.find(model => model.id === selected);
  return <div className="field">
    <label className="field-label" htmlFor={`${engine.toLowerCase()}-model-size`}>{t("{0} model size", [engine])}</label>
    <select id={`${engine.toLowerCase()}-model-size`} value={selected} disabled={disabled}
      onChange={event => onSelect(event.target.value)}>
      {models.map(model => <option key={model.id} value={model.id}>
        {message(model.label)} · {model.bytes >= 1_000_000_000 ? `${(model.bytes / 1_000_000_000).toFixed(2)} GB` : `${Math.round(model.bytes / 1_000_000)} MB`} · {model.installed ? t("Installed") : t("Download needed")}
      </option>)}
    </select>
    {onDownload && current && (!current.installed || needsDownload) ? (
      <button className="plain accent-action" disabled={disabled} onClick={onDownload}>{downloadLabel}</button>
    ) : null}
    {onDelete && current && (current.removable ?? current.installed) ? (
      <button className="plain accent-action" disabled={disabled || deleteDisabled} onClick={onDelete}>
        {deleting ? t("Deleting…") : t("Delete selected model")}
      </button>
    ) : null}
    {current ? <p className="card-note">{message(current.description)}</p> : null}
  </div>;
}
