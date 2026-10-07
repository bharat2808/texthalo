import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useAudiobooks,
  audioClock,
  listenBook,
  listenFromPage,
  generateBook,
  completeChapter,
  preparationLabel,
} from "./useAudiobooks";
import "./Audiobooks.css";
export default function Audiobooks() {
  const { state, error, action } = useAudiobooks();
  const [selected, setSelected] = useState<string | null>(null),
    [busy, setBusy] = useState(false);
  const book = state.books.find((b) => b.id === selected);
  const playing =
    state.playback && state.playback.bookId === book?.id
      ? state.playback.audio
      : null;
  const run = (fn: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    void action(fn).finally(() => setBusy(false));
  };
  const open = state.tray;
  return (
    <section className="audiobooks">
      <div className="audiobook-header">
        <div>
          <span className="audiobook-eyebrow">YOUR LISTENING LIBRARY</span>
          <h1>Audiobooks</h1>
          <p>Your books, ready to pick up where you left off.</p>
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
          <div>
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
          </div>
          <div className="audiobook-detected-actions">
            <button
              disabled={
                busy ||
                !!open.book?.pendingPage ||
                (!!state.generating && !open.book?.generated)
              }
              onClick={() =>
                run(async () => {
                  if (open.book?.generated) {
                    await listenBook(open.book.id);
                  } else {
                    const id = await generateBook(open.book?.id);
                    setSelected(id);
                  }
                })
              }
            >
              {open.book?.generated ? "Continue listening" : "Create audiobook"}
            </button>
            {open.book && open.differentPage && (
              <button
                disabled={busy}
                onClick={() =>
                  run(() =>
                    listenFromPage(open.book!.id, open.reader.fingerprint),
                  )
                }
              >
                Listen from this page
              </button>
            )}
          </div>
        </div>
      )}
      {!book ? (
        <>
          {state.books.length === 0 ? (
            <div className="audiobook-empty">
              <div className="audiobook-empty-icon" aria-hidden="true">
                ♫
              </div>
              <h2>A new way to enjoy your books</h2>
              <p>
                Open a book in Kindle for the web. TextHalo will offer to turn
                its pages into audio and remember your place.
              </p>
              <p className="audiobook-hint">
                Enable page capture in the TextHalo Reader extension once for
                the tab, then continue here.
              </p>
            </div>
          ) : (
            <div className="audiobook-grid">
              {state.books.map((b) => (
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
                    · {b.totalPages} pages
                  </p>
                  <progress
                    aria-label="Listening progress"
                    max={b.seconds || 1}
                    value={b.position}
                  />
                  <span>
                    {b.position > 0
                      ? `Continue at ${audioClock(b.position)}`
                      : "Ready to listen"}
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
              <p>
                {audioClock(book.totalSeconds)} of audio ready · {book.totalPages} pages ·{" "}
                {book.totalGenerated} sentences
              </p>
              {book.resume && (
                <p className="audiobook-location">
                  Last listened:{" "}
                  {book.resume.chapter ? `${book.resume.chapter} · ` : ""}
                  {book.resume.page}
                </p>
              )}
              {open?.book?.id === book.id && (
                <p className="audiobook-location">
                  Open in Kindle: {open.reader.progress}
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
                <button
                  className="primary"
                  disabled={
                    busy ||
                    !book.generated ||
                    (!!book.pendingPage && (!playing || playing.paused))
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
                </button>
                {open?.book?.id === book.id && open.differentPage && (
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
                  min="0"
                  max={book.seconds || 1}
                  step="0.1"
                  value={playing?.position ?? book.position}
                  disabled={!book.generated || busy}
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
                  <span>{audioClock(playing?.position ?? book.position)}</span>
                  <span>
                    {audioClock(book.seconds)}
                    {book.complete ? "" : " buffered"}
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
                        busy || !!state.generating || open?.book?.id !== book.id
                      }
                      onClick={() => run(() => generateBook(book.id))}
                    >
                      {book.pendingPage ? "Resume preparing" : "Buffer as I listen"}
                    </button>
                    {book.checkpoints[0] && !book.checkpoints[0].complete && (
                      <button
                        disabled={
                          busy ||
                          !!state.generating ||
                          open?.book?.id !== book.id
                        }
                        onClick={() => run(() => completeChapter(book.id))}
                      >
                        {book.backfillBlocked
                          ? "Retry chapter preparation"
                          : "Complete first saved chapter"}
                      </button>
                    )}
                    <button
                      disabled={
                        busy ||
                        !!state.generating ||
                        open?.book?.id !== book.id
                      }
                      title="Fill missing passages and reuse audio already prepared"
                      onClick={() => run(() => generateBook(book.id, true))}
                    >
                      Create full audiobook
                    </button>
                  </div>
                )
              )}
              {!book.fromBeginning && (
                <p className="audiobook-hint">
                  {book.checkpoints[0]?.needsEarlier
                    ? "Listening starts at your imported page. Earlier pages of this chapter are filled while your listening buffer is ready."
                    : "Create full audiobook fills earlier gaps and missing passages while keeping your saved audio."}
                </p>
              )}
            </div>
          </div>
          <div className="audiobook-chapters">
            <h3>Chapters and checkpoints</h3>
            <p>
              Partial chapters stay here so you can continue listening or finish
              preparing them.
            </p>
            {book.checkpoints.map((c) => (
              <button
                key={c.id}
                disabled={c.seconds === null || busy}
                onClick={() =>
                  run(() => listenBook(book.id, c.seconds!, c.rangeId))
                }
              >
                <span>
                  {c.label}
                  <small
                    className={`chapter-status chapter-status-${c.status}`}
                  >
                    {c.status === "attention"
                      ? "Needs attention"
                      : c.status === "complete"
                        ? "Complete"
                        : c.status === "preparing"
                          ? "Preparing"
                          : "Partial · ready to continue"}
                  </small>
                </span>
                <small>
                  {c.page} ·{" "}
                  {c.seconds === null
                    ? "Preparing"
                    : `${audioClock(c.duration)} ready`}
                  {c.needsEarlier && " · earlier pages pending"}
                </small>
                <span aria-hidden="true">▶</span>
              </button>
            ))}
          </div>
          {book.passage && (
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
