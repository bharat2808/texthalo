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
  activity: "forward" | "backfill" | "selected" | "locating" | null;
  resume: { page: string; chapter: string | null } | null;
  pendingPage: string | null;
  backfillBlocked: string | null;
  backfillProgress: {
    wholeBook?: boolean;
    chapter: string | null;
    phase: "searching" | "rendering" | "publishing";
    pages: number;
    ready: number;
    total: number | null;
  } | null;
  checkpoints: Checkpoint[];
  passage: { text: string; page: string } | null;
};
export type Audio = {
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

export function completeChapter(id: string) {
  return invoke<string>("audiobook_complete_chapter", { id });
}

export function preparationLabel(book: Audiobook) {
  const progress = book.backfillProgress;
  if (book.activity === "locating") return book.recovery?.label || "Locating your saved sentence…";
  if (book.pendingPage && book.activity === null) {
    return `Selected page saved · ${book.pendingPage}. Resume preparing to listen.`;
  }
  if (book.pendingPage || book.activity === "selected") {
    const page = book.pendingPage ? ` · ${book.pendingPage}` : "";
    const sentences =
      progress?.total != null
        ? ` · ${progress.ready}/${progress.total} sentences`
        : "";
    return `Preparing selected page…${page}${sentences}`;
  }
  if (!progress) return "Preparing pages ahead of you";
  const chapter = progress.wholeBook ? "audiobook" : progress.chapter || "chapter";
  if (book.activity === "forward") {
    return `Preparing pages ahead · ${progress.pages} earlier pages saved`;
  }
  if (progress.phase === "searching") {
    return progress.wholeBook
      ? `Finding missing earlier passages · ${progress.pages} pages captured`
      : `Finding chapter beginning · ${progress.pages} earlier pages captured`;
  }
  if (progress.phase === "publishing")
    return `Adding earlier audio to ${chapter}`;
  if (progress.wholeBook && progress.total !== null)
    return `Generating earlier audio · ${progress.ready}/${progress.total} sentences`;
  return progress.total === null
    ? `Preparing ${chapter} · ${progress.pages} earlier pages captured`
    : `Preparing ${chapter} · ${progress.ready}/${progress.total} sentences`;
}
