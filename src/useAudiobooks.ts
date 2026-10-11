import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
export type Checkpoint = {
  id: string;
  rangeId: string;
  status: "complete" | "partial" | "preparing" | "attention";
  duration: number;
  complete: boolean;
  needsEarlier: boolean;
  label: string;
  sentence: number;
  page: string;
  seconds: number | null;
};
export type Audiobook = {
  id: string;
  key: string;
  title: string;
  platform: string;
  pages: number;
  generated: number;
  totalPages: number;
  totalGenerated: number;
  totalSeconds: number;
  generatedSeconds?: number;
  seconds: number;
  position: number;
  speed: number;
  complete: boolean;
  fromBeginning: boolean;
  message: string | null;
  recovery?: {
    step: "searching" | "matched" | "unresolved";
    label: string;
    nearby?: { sentenceId: string; seconds: number; page: string } | null;
  } | null;
  activity: "forward" | "selected" | "locating" | "full-scan" | null;
  fullPreparation?: {chapters: number; completedChapters: number; transcriptReady: boolean; total: number; phase: string; pages: number; page: string | null; generated: number; reused: number} | null;
  resume: { page: string; chapter: string | null; seconds: number; rangeId: string } | null;
  pendingPage: string | null;
  checkpoints: Checkpoint[];
  passage: { text: string; page: string } | null;
};
export type Audio = {
  start: number;
  position: number;
  buffered: number;
  duration: number | null;
  paused: boolean;
  speed: number;
  available: boolean;
};
export type AudiobookState = {
  books: Audiobook[];
  tray: {
    reader: {
      title: string;
      progress: string;
      bookId: string;
      url: string;
      fingerprint: string;
    };
    book: Audiobook | null;
    generating: boolean;
    differentPage: boolean;
  } | null;
  readerContext?: AudiobookState["tray"];
  generating: string | null;
  readerWarning?: { bookId: string; code: "chrome-minimized" } | null;
  playback: { bookId: string; audio: Audio } | null;
};
const empty: AudiobookState = {
  books: [],
  tray: null,
  generating: null,
  playback: null,
};
export function useAudiobooks() {
  const [state, setState] = useState(empty),
    [error, setError] = useState(""),
    [connectionError, setConnectionError] = useState("");
  useEffect(() => {
    let mounted = true,
      polling = false;
    const refresh = async () => {
      if (polling) return;
      polling = true;
      try {
        const next = await invoke<AudiobookState>("audiobook_state");
        if (mounted) {
          setState(next);
          setConnectionError("");
        }
      } catch (e) {
        if (mounted) setConnectionError(String(e));
      } finally {
        polling = false;
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 750);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  }, []);
  const action = async (fn: () => Promise<unknown>) => {
    setError("");
    try {
      await fn();
      setState(await invoke<AudiobookState>("audiobook_state"));
    } catch (e) {
      setError(String(e));
    }
  };
  return { state, error: error || connectionError, action };
}
export const audioClock = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
export function listenBook(id: string, seconds?: number, rangeId?: string) {
  return invoke("audiobook_play", {
    id,
    seconds: seconds ?? null,
    rangeId: rangeId ?? null,
  });
}
export function listenFromPage(id: string, fingerprint: string) {
  return invoke<string>("audiobook_listen_page", { id, fingerprint });
}
export function generateBook(id?: string, full = false) {
  return invoke<string>("audiobook_generate", { id: id ?? null, full });
}

export function preparationLabel(book: Audiobook) {
  const full = book.fullPreparation;
  if (full && book.activity === "full-scan") {
    if (full.phase === "finding-start") return "Finding the beginning of the book…";
    if (full.phase === "publishing") return "Saving the complete audiobook…";
    if (!full.transcriptReady) return `Capturing transcript · ${full.pages} pages saved · ${full.page || "starting"}`;
    return `Generating audiobook · ${full.completedChapters}/${full.chapters} chapters ready`;
  }
  if (book.activity === "locating") return book.recovery?.label || "Locating the selected passage…";
  if (book.pendingPage) return `Preparing selected page · ${book.pendingPage}`;
  return "Preparing the current chapter…";
}
