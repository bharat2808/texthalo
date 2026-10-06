import { t } from "./i18n";
import { type ReactNode } from "react";
import { getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";

export default function OverlayGrip({ children, onError }: { children?: ReactNode; onError?: (message: string) => void }) {
  const report = (cause: unknown) => onError?.(String(cause));
  return <div className="overlay-grip">
    <div className="overlay-grip-indicator" aria-hidden="true">{children}</div>
    <button className="overlay-move" aria-label={t("Move widget")} title={t("Drag to move · Arrow keys to adjust")}
      onMouseDown={event => {
        if (event.button !== 0) return;
        event.preventDefault();
        void getCurrentWindow().startDragging().catch(report);
      }}
      onKeyDown={event => {
        const direction = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[event.key];
        if (!direction) return;
        event.preventDefault();
        const window = getCurrentWindow();
        void Promise.all([window.outerPosition(), window.scaleFactor()]).then(([position, scale]) => {
          const step = (event.shiftKey ? 50 : 10) * scale;
          return window.setPosition(new PhysicalPosition(position.x + direction[0] * step, position.y + direction[1] * step));
        }).catch(report);
      }}>
      <svg width="14" height="16" viewBox="0 0 14 16" fill="currentColor" aria-hidden="true">
        {[4, 8, 12].flatMap(y => [4, 10].map(x => <circle key={`${x}-${y}`} cx={x} cy={y} r="1.3" />))}
      </svg>
    </button>
  </div>;
}
