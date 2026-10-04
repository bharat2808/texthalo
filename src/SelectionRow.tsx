import type React from "react";

export default function Row({
  selected,
  glyph,
  title,
  subtitle,
  mono,
  badge,
  disabled,
  onSelect,
  trailing,
}: {
  selected: boolean;
  glyph?: React.ReactNode;
  title: string;
  subtitle?: string;
  mono?: boolean;
  badge?: string;
  disabled?: boolean;
  onSelect: () => void;
  trailing?: React.ReactNode;
}) {
  return (
    <div
      className={selected ? "sel-row selected" : "sel-row"}
      role="button"
      tabIndex={disabled ? -1 : 0}
      aria-pressed={selected}
      /* `:disabled` cannot match a div, so the attribute is what carries the state. */
      aria-disabled={disabled ? true : undefined}
      onClick={disabled ? undefined : onSelect}
      onKeyDown={(event) => {
        if (disabled || event.target !== event.currentTarget) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      <span className="glyph">{glyph ?? <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
        <path d="M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14m0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16" />
        {selected && <path d="m10.97 4.97-.02.022-3.473 4.425-2.093-2.094a.75.75 0 0 0-1.06 1.06L6.97 11.03a.75.75 0 0 0 1.079-.02l3.992-4.99a.75.75 0 0 0-1.071-1.05" />}
      </svg>}</span>
      <span className="sel-text">
        <span className={mono ? "sel-title mono" : "sel-title"}>{title}</span>
        {subtitle ? <span className="sel-subtitle">{subtitle}</span> : null}
      </span>
      {badge ? <span className="sel-badge">{badge}</span> : null}
      {trailing}
    </div>
  );
}
