import { useEffect, useLayoutEffect, useRef } from "react";
import { watchNativeHover } from "./nativeHover";

export default function LiveTextPreview({ text, expanded, label, pending, toggle, resize }: {
  text: string; expanded: boolean; label: string; pending: boolean; toggle: () => void; resize: (height: number) => void;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  const expandedOnHover = useRef(false);
  const expandButton = useRef<HTMLButtonElement>(null);
  const toggleRef = useRef(toggle);
  const resizeRef = useRef(resize);
  const measuredHeight = useRef<number | null>(null);
  const collapseRequested = useRef(false);
  toggleRef.current = toggle;
  resizeRef.current = resize;
  useLayoutEffect(() => {
    const element = viewport.current;
    const content = element?.firstElementChild;
    if (!expanded || !element || !content) { measuredHeight.current = null; return; }
    const measure = () => {
      const widget = element.closest(".speech-overlay");
      if (!widget) return;
      const chrome = widget.getBoundingClientRect().height - element.getBoundingClientRect().height;
      const height = Math.max(76, Math.ceil(chrome + content.getBoundingClientRect().height));
      if (height === measuredHeight.current) return;
      measuredHeight.current = height;
      resizeRef.current(height);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => observer.disconnect();
  }, [expanded, text]);
  useEffect(() => {
    if (!expanded) {
      collapseRequested.current = false;
      expandedOnHover.current = false;
    }
    if (pending || !expandButton.current) return;
    if (expanded) {
      const widget = expandButton.current.closest<HTMLElement>(".speech-overlay");
      if (!widget) return;
      const collapse = () => {
        if (collapseRequested.current) return;
        collapseRequested.current = true;
        expandedOnHover.current = false;
        toggleRef.current();
      };
      widget.addEventListener("pointerleave", collapse);
      const stop = watchNativeHover(widget, () => {}, collapse);
      return () => { widget.removeEventListener("pointerleave", collapse); stop(); };
    }
    return watchNativeHover(expandButton.current, () => {
      if (expandedOnHover.current) return;
      expandedOnHover.current = true;
      toggleRef.current();
    });
  }, [expanded, pending]);
  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    element.classList.toggle("has-overflow", !expanded && element.scrollWidth > element.clientWidth);
    if (expanded) element.scrollTop = element.scrollHeight;
    else element.scrollLeft = element.scrollWidth;
  }, [text, expanded]);
  return <div className={`live-text-preview${expanded ? " expanded" : ""}`}>
    <div ref={viewport} className="live-text-viewport" aria-label={label}>
      <div className="live-text-content">{expanded ? text : text.replace(/\s+/g, " ")}</div>
    </div>
    <button ref={expandButton} className="preview-expand" aria-label={expanded ? "Collapse live text" : "Expand live text"}
      aria-expanded={expanded} title={expanded ? "Show one line" : "Wrap into a passage"}
      disabled={pending}
      onPointerMove={event => {
        if (event.movementX === 0 && event.movementY === 0) return;
        if (!expanded && !pending && !expandedOnHover.current) {
          expandedOnHover.current = true;
          toggle();
        }
      }}
      onMouseLeave={() => { expandedOnHover.current = false; }}
      onClick={() => {
        // A click immediately following hover should keep the passage open.
        if (expandedOnHover.current) { expandedOnHover.current = false; return; }
        toggle();
      }}>
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
        <path d={expanded ? "M1 5h4V1M5 5 1 1M15 11h-4v4M11 11l4 4" : "M1 6V1h5M1 1l5 5M15 10v5h-5M15 15l-5-5"} />
      </svg>
    </button>
  </div>;
}
