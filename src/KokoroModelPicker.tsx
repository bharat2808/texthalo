
export type KokoroModelOption = {
  id: string;
  label: string;
  description: string;
  bytes: number;
  installed: boolean;
};

export default function KokoroModelPicker({ models, selected, disabled, onSelect, engine = "Kokoro" }: {
  engine?: "Kokoro" | "Chatterbox";
  models: KokoroModelOption[];
  selected: string;
  disabled: boolean;
  onSelect: (id: string) => void;
}) {
  const current = models.find(model => model.id === selected);
  return <div className="field">
    <label className="field-label" htmlFor={`${engine.toLowerCase()}-model-size`}>{engine} model size</label>
    <select id={`${engine.toLowerCase()}-model-size`} value={selected} disabled={disabled}
      onChange={event => onSelect(event.target.value)}>
      {models.map(model => <option key={model.id} value={model.id}>
        {model.label} · {model.bytes >= 1_000_000_000 ? `${(model.bytes / 1_000_000_000).toFixed(2)} GB` : `${Math.round(model.bytes / 1_000_000)} MB`} · {model.installed ? "Installed" : "Download needed"}
      </option>)}
    </select>
    {current ? <p className="card-note">{current.description}</p> : null}
  </div>;
}
