import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import {
  useAudiobooks,
  audioClock,
  listenBook,
  listenFromPage,
  generateBook,
  preparationLabel,
} from "./useAudiobooks";
import "./Audiobooks.css";
export default function Audiobooks() {
  const { state, error, action } = useAudiobooks();
  const [selected, setSelected] = useState<string | null>(null),
    [busy, setBusy] = useState(false);
  const libraryBooks = state.books.filter((b) =>
    b.checkpoints.some((c) => c.complete && c.seconds !== null) ||
    state.playback?.bookId === b.id || state.generating === b.id || b.fullPreparation,
  );
  const book = libraryBooks.find((b) => b.id === selected);
  const chapters = book?.checkpoints.filter((c) => c.complete && c.seconds !== null) ?? [];
  const playing =
    state.playback && state.playback.bookId === book?.id
      ? state.playback.audio
      : null;
  const resumeChapters = chapters.filter((c) => c.rangeId === book?.resume?.rangeId);
  const timelineEnd = playing?.buffered ?? (resumeChapters.length
    ? Math.max(...resumeChapters.map((c) => c.seconds! + c.duration))
    : book?.totalSeconds ?? 0);
  const timelinePosition = playing?.position ?? book?.resume?.seconds ?? book?.position ?? 0;
  const run = (fn: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    void action(fn).finally(() => setBusy(false));
  };
  const open = state.readerContext ?? state.tray;
  const extensionUrl = "https://chromewebstore.google.com/detail/texthalo-reader/jafnpokahhbldophkeogljemkblfmgmo";
  return (
    <section className="audiobooks">
      <div className="audiobook-header">
        <div>
          <span className="audiobook-eyebrow">YOUR LISTENING LIBRARY</span>
          <h1>Audiobooks</h1>
          <p>Your books, ready to pick up where you left off.</p>
          <button disabled={busy} onClick={() => run(() => openUrl(extensionUrl))}>
            Get TextHalo Reader from the Chrome Web Store ↗
          </button>
          <p className="audiobook-hint">Add the extension to Chrome, open your Kindle book, then choose Connect to TextHalo in the extension.</p>
        </div>
      </div>
      {error && (
        <p className="audiobook-error" role="alert">
          {error}
        </p>
      )}
      {open && (
        <div className="audiobook-detected">
          <div className="kindle-mark" aria-label="Kindle">
            k
          </div>
          <div className="audiobook-detected-details">
            <span>OPEN IN KINDLE · {open.reader.progress}</span>
            <strong>{open.reader.title || "Kindle book"}</strong>
            {open.book?.resume && (
              <p className="audiobook-location">
                Last listened:{" "}
                {open.book.resume.chapter
                  ? `${open.book.resume.chapter} · `
                  : ""}
                {open.book.resume.page}
              </p>
            )}
            {open.book?.pendingPage && (
              <p role="status">{preparationLabel(open.book)}</p>
            )}
            <p className="audiobook-hint">
              Connected · Choose where to start or build the full audiobook in
              TextHalo Reader.
            </p>
          </div>
        </div>
      )}
      {!book ? (
        <>
          {libraryBooks.length === 0 ? (
            <div className="audiobook-empty">
              <div className="audiobook-empty-icon" aria-hidden="true">
                ♫
              </div>
              <h2>A new way to enjoy your books</h2>
              <p>
                Open a book in Kindle for the web, then connect using TextHalo Reader
                in Chrome to turn its pages into audio and remember your place.
              </p>
              <p className="audiobook-hint">
                Start from a page or build the full audiobook in the extension.
                Playback controls and your saved library remain in TextHalo.
              </p>
            </div>
          ) : (
            <div className="audiobook-grid">
              {libraryBooks.map((b) => (
                <button
                  className="audiobook-tile"
                  key={b.id}
                  onClick={() => setSelected(b.id)}
                >
                  <div className="audiobook-cover" aria-hidden="true">
                    <span>kindle</span>
                    <strong>{b.title}</strong>
                    <i>TextHalo Audiobooks</i>
                  </div>
                  <h2>{b.title}</h2>
                  <p>
                    {b.pendingPage || state.generating === b.id
                      ? preparationLabel(b)
                      : b.complete
                        ? "Complete audiobook"
                        : `${audioClock(b.totalSeconds)} ready`}{" "}
                    · {b.totalPages} captured views
                  </p>
                  <progress
                    aria-label="Listening progress"
                    max={b.totalSeconds || 1}
                    value={b.resume?.seconds ?? b.position}
                  />
                  <span>
                    {b.resume
                      ? `Continue ${b.resume.chapter || "completed chapter"}`
                      : "No completed chapters yet"}
                  </span>
                </button>
              ))}
            </div>
          )}
        </>
      ) : (
        <>
          <button className="audiobook-back" onClick={() => setSelected(null)}>
            ← All audiobooks
          </button>
          <div className="audiobook-detail">
            <div className="audiobook-cover" aria-hidden="true">
              <span>kindle</span>
              <strong>{book.title}</strong>
              <i>TextHalo Audiobooks</i>
            </div>
            <div className="audiobook-info">
              <span className="audiobook-eyebrow">
                KINDLE · {book.complete ? "COMPLETE" : "GROWING AUDIOBOOK"}
              </span>
              <h2>{book.title}</h2>
              <p>{audioClock(book.totalSeconds)} ready</p>
              {book.resume && (
                <p className="audiobook-location" title={book.resume.page}>
                  {playing ? "Listening" : "Continue"}: {book.resume.chapter || book.resume.page}
                </p>
              )}
              {book.pendingPage && state.generating !== book.id && (
                <p role="status">{preparationLabel(book)}</p>
              )}
              {book.message && (
                <p className="audiobook-error" role="status">
                  {book.message}
                </p>
              )}
              <div className="audiobook-actions">
                {(playing || book.resume) && <button
                  className="primary"
                  disabled={
                    busy ||
                    !chapters.length ||
                    (!!book.pendingPage && !book.fullPreparation && (!playing || playing.paused))
                  }
                  onClick={() =>
                    run(() =>
                      playing
                        ? invoke("playback_control", {
                            command: playing.paused ? "resume" : "pause",
                          })
                        : listenBook(book.id),
                    )
                  }
                >
                  {playing
                    ? playing.paused
                      ? "Resume listening"
                      : "Pause listening"
                    : book.resume || book.position
                      ? "Continue listening"
                      : "Play"}
                </button>}
                {open?.book?.id === book.id && (
                  <button
                    disabled={busy}
                    onClick={() =>
                      run(() =>
                        listenFromPage(book.id, open.reader.fingerprint),
                      )
                    }
                  >
                    Listen from this page
                  </button>
                )}
                {playing && (
                  <button onClick={() => run(() => invoke("stop_speaking"))}>
                    Stop listening
                  </button>
                )}
                <label>
                  Speed{" "}
                  <select
                    disabled={!playing || busy}
                    value={playing?.speed ?? book.speed}
                    onChange={(e) =>
                      run(() =>
                        invoke("playback_control", {
                          command: "speed",
                          value: Number(e.target.value),
                        }),
                      )
                    }
                  >
                    {[0.75, 1, 1.25, 1.5, 1.75, 2].map((s) => (
                      <option key={s} value={s}>
                        {s}×
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <div className="audiobook-timeline">
                <input
                  aria-label="Audiobook playback position"
                  type="range"
                  min={playing?.start ?? 0}
                  max={timelineEnd || 1}
                  step="0.1"
                  value={Math.min(timelinePosition, timelineEnd)}
                  disabled={!playing || busy}
                  onChange={(e) =>
                    run(() =>
                      playing
                        ? invoke("playback_control", {
                            command: "seek",
                            value: Number(e.target.value),
                          })
                        : listenBook(book.id, Number(e.target.value)),
                    )
                  }
                />
                <div>
                  <span>{audioClock(timelinePosition)}</span>
                  <span>
                    {audioClock(timelineEnd)}
                    {book.complete ? "" : " playable"}
                  </span>
                </div>
              </div>
              {state.generating === book.id ? (
                <div className="audiobook-buffer">
                  <span className="audiobook-pulse" />
                  <span role="status">{preparationLabel(book)}</span>
                  <button
                    onClick={() =>
                      run(() => invoke("audiobook_stop_generation"))
                    }
                  >
                    Stop preparing
                  </button>
                </div>
              ) : (
                !book.complete && (
                  <div className="audiobook-actions">
                    <button
                      disabled={
                        busy ||
                        !!state.generating ||
                        (!book.fullPreparation?.transcriptReady && open?.book?.id !== book.id)
                      }
                      title="Fill missing passages and reuse audio already prepared"
                      onClick={() => run(() => generateBook(book.id, true))}
                    >
                      {book.fullPreparation?.transcriptReady ? "Prepare more chapters" : book.fullPreparation ? "Resume full audiobook" : "Create full audiobook"}
                    </button>
                  </div>
                )
              )}
              {!book.fromBeginning && !book.fullPreparation && (
                <p className="audiobook-hint">
                  Listening starts at your selected page. Only chapters recorded
                  from beginning to end are available to continue later.
                </p>
              )}
            </div>
          </div>
          <div className="audiobook-chapters">
            <h3>Completed chapters</h3>
            {state.generating === book.id && (
              <p role="status">Finished chapters appear here as they become ready. You can listen while the rest of the book is being prepared.</p>
            )}
            {!chapters.length && <p>No completed chapters yet.</p>}
            {chapters.map((c) => (
              <button
                key={`${c.rangeId}:${c.id}`}
                disabled={busy}
                onClick={() =>
                  run(() => listenBook(book.id, c.seconds!, c.rangeId))
                }
              >
                <span>
                  {c.label}
                  <small
                    className={`chapter-status chapter-status-${c.status}`}
                  >
                    Complete
                  </small>
                </span>
                <small>
                  {c.page} ·{" "}
                  {audioClock(c.duration)} ready
                </small>
                <span aria-hidden="true">▶</span>
              </button>
            ))}
          </div>
          {playing && book.passage && (
            <div className="audiobook-passage">
              <span className="audiobook-eyebrow">
                CURRENT SENTENCE · {book.passage.page}
              </span>
              <p>{book.passage.text}</p>
            </div>
          )}
        </>
      )}
    </section>
  );
}
