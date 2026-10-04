import { isTauri } from "@tauri-apps/api/core";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";

// Non-key floating macOS windows may not deliver DOM hover while a hold key is
// down. Compare native pointer coordinates with the actual control bounds.
export function watchNativeHover(element: HTMLElement, enter: () => void, leave?: () => void): () => void {
  if (!isTauri()) return () => {};
  const window = getCurrentWindow();
  let disposed = false;
  let checking = false;
  let entered = false;
  let wasInside: boolean | null = null;
  let previousCursor: { x: number; y: number } | null = null;
  const check = async () => {
    if (disposed || checking) return;
    checking = true;
    try {
      const [cursor, origin, scale] = await Promise.all([cursorPosition(), window.innerPosition(), window.scaleFactor()]);
      if (disposed || !element.isConnected || scale <= 0) return;
      const bounds = element.getBoundingClientRect();
      const x = (cursor.x - origin.x) / scale;
      const y = (cursor.y - origin.y) / scale;
      const hit = x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom;
      const moved = previousCursor !== null && (cursor.x !== previousCursor.x || cursor.y !== previousCursor.y);
      // Appearing/resizing under a stationary pointer is not an intentional hover.
      if (hit && moved && !entered) { entered = true; enter(); }
      if (!hit) entered = false;
      if (!hit && wasInside !== false) leave?.();
      wasInside = hit;
      previousCursor = cursor;
    } catch {
      // DOM hover still works if native pointer access is unavailable.
      disposed = true;
      globalThis.clearInterval(timer);
    } finally { checking = false; }
  };
  const timer = globalThis.setInterval(() => void check(), 120);
  void check();
  return () => { disposed = true; globalThis.clearInterval(timer); };
}
