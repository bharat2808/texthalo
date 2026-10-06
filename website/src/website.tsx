import { useWebsiteI18n } from "./i18n";
import { DEMO_VIDEO } from "./demo";
import { SiteHeader } from "./billing";
import { MAC_DOWNLOAD as DOWNLOAD } from "./downloads";
import { version as sourceVersion } from "../../package.json";

const version = import.meta.env.VITE_TEXTHALO_DOWNLOAD_VERSION || sourceVersion;


function Mark({ small = false }: { small?: boolean }) {
  return (
    <span className={`brand-mark${small ? " brand-mark-small" : ""}`} aria-hidden="true">
      <span />
    </span>
  );
}

function Arrow() {
  return <span aria-hidden="true" className="arrow">→</span>;
}

function ProductVideo() {
  const i18n = useWebsiteI18n();
  return (
    <div className="product-video">
      <div className="product-video-label"><span className="live-dot" /> {i18n.t("SEE TEXTHALO IN ACTION")}</div>
      <video
        controls
        playsInline
        preload="metadata"
        poster={DEMO_VIDEO.poster}
        aria-label={i18n.t("30-second animated TextHalo demo: Listen, Dictate, and Assistant")}
      >
        <source src={DEMO_VIDEO.src} type="video/mp4" />
        <track kind="captions" src={DEMO_VIDEO.captions} srcLang="en" label="English" />
        {i18n.t("Your browser does not support embedded video.")}</video>
      <div className="product-video-caption"><span>{i18n.t("30 SECOND WORKFLOW DEMO")}</span><span>{i18n.t("VOICEOVER · FISH AUDIO")}</span></div>
      <a className="video-watch-link" href={i18n.href("/demo/")}>{i18n.t("Watch the product demo")} <Arrow /></a>
    </div>
  );
}

function Website() {
  const i18n = useWebsiteI18n();
  return (
    <div className="site-shell">
      <SiteHeader />

      <main id="top">
        <section className="hero section-wrap">
          <div className="hero-copy">
            <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("LISTEN. DICTATE. GET A LITTLE HELP.")}</div>
            <h1>{i18n.t("Your words.")}<br /><em>{i18n.t("More possibilities.")}</em></h1>
            <p className="hero-text">{i18n.t("Listen to selected text, turn speech into writing, or ask Assistant to write, edit, and read aloud. All from a shortcut on your Mac.")}</p>
            <div className="hero-actions">
              <a className="button button-dark button-large" href={i18n.href(DOWNLOAD)} target="_blank" rel="noreferrer">{i18n.t("Download for Mac")} <Arrow /></a>
              <a className="text-link" href={i18n.href("#how-it-works")}>{i18n.t("See how it works")} <Arrow /></a>
            </div>
            <div className="hero-meta"><span><i className="apple-mark">●</i> {i18n.t("Requires Apple Silicon")}</span><span className="meta-divider" /><span>{i18n.t("Free local features")}</span></div>
          </div>
          <ProductVideo />
          <div className="hero-footnote"><span>01 / 03</span><span className="footnote-line" /><span>{i18n.t("ONE MAC APP, THREE WAYS TO WORK")}</span></div>
        </section>

        <section className="ticker" aria-label={i18n.t("TextHalo benefits")}>
          <div>{i18n.t("LISTEN")} <b>✳</b> {i18n.t("DICTATE")} <b>✳</b> {i18n.t("ASSISTANT")} <b>✳</b> {i18n.t("LISTEN")} <b>✳</b> {i18n.t("DICTATE")} <b>✳</b> {i18n.t("ASSISTANT")} <b>✳</b></div>
        </section>

        <section id="how-it-works" className="how section-wrap">
          <div className="section-heading"><div><div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("THREE WAYS TO WORK")}</div><h2>{i18n.t("Read less. Say more.")}<br /><em>{i18n.t("Keep your flow.")}</em></h2></div><p>{i18n.t("TextHalo lives in your menu bar. Choose your shortcuts and work in the app you already have open.")}</p></div>
          <div className="steps-grid">
            <article className="step-card"><div className="step-top"><span>{i18n.t("01 / LISTEN")}</span><span className="step-icon sound-icon">≈</span></div><h3>{i18n.t("Hear your words")}</h3><p>{i18n.t("Select an article, a draft, or an email. Press your shortcut to hear it aloud, with a floating player to stop playback.")}</p><div className="mini-shortcut"><span>{i18n.t("Read selection")}</span><kbd>⌘</kbd><kbd>⇧</kbd><kbd>S</kbd></div></article>
            <article className="step-card"><div className="step-top"><span>{i18n.t("02 / DICTATE")}</span><span className="step-icon selection-icon">⌖</span></div><h3>{i18n.t("Speak instead of typing")}</h3><p>{i18n.t("Hold your dictation shortcut, speak, then release. Insert your words into the current app or copy them to the clipboard.")}</p><div className="mini-selection">{i18n.t("“Here’s what I have in mind.”")} <i /></div></article>
            <article className="step-card step-card-accent"><div className="step-top"><span>{i18n.t("03 / ASSISTANT")}</span><span className="step-icon chatter-icon">✳</span></div><h3>{i18n.t("Ask for a little help")}</h3><p>{i18n.t("Hold your Assistant shortcut to ask for writing, an edit, or something read aloud. Use selected text and available window context.")}</p><div className="mini-selection">{i18n.t("“Make this paragraph clearer.”")} <i /></div></article>
          </div>
          <p className="workflow-note">{i18n.t("Listen starts with ⌘⇧S. Set up your own Dictate and Assistant shortcuts in the app.")}</p>
        </section>

        <section id="voices" className="voices-section">
          <div className="voices section-wrap">
            <div className="voices-copy"><div className="eyebrow eyebrow-light"><span className="eyebrow-line" /> {i18n.t("A VOICE THAT FITS THE MOMENT")}</div><h2>{i18n.t("Four ways to")}<br /><em>{i18n.t("hear it.")}</em></h2><p>{i18n.t("Start with Apple voices, download a local AI engine, or sign in for Fish Audio hosted voices. Preview a voice before you choose it.")}</p></div>
            <div className="engine-list">
              <article className="engine-card engine-apple"><div className="engine-number">01</div><div className="engine-main"><div className="engine-icon system-icon">◖</div><div><h3>{i18n.t("Apple voices")}</h3><p>{i18n.t("Ready when you are. Uses voices installed on your Mac.")}</p></div></div><span className="engine-tag">{i18n.t("READY TO GO")}</span></article>
              <article className="engine-card"><div className="engine-number">02</div><div className="engine-main"><div className="engine-icon kokoro-icon">k</div><div><h3>Kokoro</h3><p>{i18n.t("Lightweight, natural-sounding speech that runs locally.")}</p></div></div><span className="engine-tag">{i18n.t("LOCAL AI")}</span></article>
              <article className="engine-card"><div className="engine-number">03</div><div className="engine-main"><div className="engine-icon chatter-icon">✳</div><div><h3>Chatterbox</h3><p>{i18n.t("Expressive multilingual speech with your own reference voice.")}</p></div></div><span className="engine-tag">{i18n.t("LOCAL AI")}</span></article>
              <article className="engine-card"><div className="engine-number">04</div><div className="engine-main"><div className="engine-icon system-icon">≈</div><div><h3>Fish Audio</h3><p>{i18n.t("Stream hosted voices and create saved voice clones with eligible access.")}</p></div></div><span className="engine-tag">{i18n.t("HOSTED · CREDITS")}</span></article>
              <p className="engine-footnote"><span>↳</span> {i18n.t("Kokoro and Chatterbox need model downloads. Fish Audio needs sign-in, credits, and your hosted-text acknowledgement.")}</p>
            </div>
          </div>
          <div className="dark-orbit dark-orbit-a" /><div className="dark-orbit dark-orbit-b" />
        </section>

        <section id="privacy" className="privacy section-wrap">
          <div className="privacy-art"><div className="privacy-ring ring-a"/><div className="privacy-ring ring-b"/><div className="privacy-center"><Mark /><span>{i18n.t("ON YOUR MAC")}</span></div><div className="privacy-pill pill-top">{i18n.t("LOCAL SPEECH")}</div><div className="privacy-pill pill-bottom">{i18n.t("LOCAL OR HOSTED")}</div></div>
          <div className="privacy-copy"><div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("YOUR WORDS, YOUR CHOICE")}</div><h2>{i18n.t("Privacy by")}<br /><em>{i18n.t("where it happens.")}</em></h2><p>{i18n.t("Local voices and dictation process on your Mac. On-device Assistant uses Apple Intelligence when available. Cloud Assistant sends your request and text context to TextHalo and its model provider; Fish Audio separately receives the words you ask it to speak.")}</p><div className="privacy-points"><div><span className="check-mark">✓</span><span><strong>{i18n.t("Choose where processing happens")}</strong><small>{i18n.t("Use local speech and dictation, or enable hosted services with their privacy notices.")}</small></span></div><div><span className="check-mark">✓</span><span><strong>{i18n.t("Optional features stay optional")}</strong><small>{i18n.t("Cloud Assistant requires consent. Assistant screenshots and microphone audio are not uploaded for cloud generation.")}</small></span></div></div><a className="text-link" href={i18n.href("/privacy/")}>{i18n.t("Read the privacy policy")} <Arrow /></a></div>
        </section>

        <section className="closing-cta"><div className="closing-inner"><div className="closing-mark"><Mark /></div><div className="eyebrow eyebrow-light"><span className="eyebrow-line" /> {i18n.t("READY WHEN YOU ARE")}</div><h2>{i18n.t("Listen. Dictate.")}<br /><em>{i18n.t("Get a little help.")}</em></h2><p>{i18n.t("Free local features. Optional paid Cloud Assistant and hosted voices.")}</p><a className="button button-cream button-large" href={i18n.href(DOWNLOAD)} target="_blank" rel="noreferrer">{i18n.t("Get TextHalo for macOS")} <Arrow /></a><span className="cta-version">{i18n.t("Current version {0}", [version])} <span>·</span> {i18n.t("Requires Apple Silicon")}</span></div><div className="cta-decoration cta-dec-a"/><div className="cta-decoration cta-dec-b"/></section>
      </main>

      <footer className="site-footer"><a className="wordmark footer-wordmark" href={i18n.href("#top")}><Mark /><span>TextHalo</span></a><span className="footer-copy">{i18n.t("Listen. Dictate. Get a little help.")}</span><div className="footer-links"><a href={i18n.href("/pricing/")}>{i18n.t("Pricing")}</a><a href={i18n.href("/demo/")}>{i18n.t("Demo")}</a><a href={i18n.href("/stories/")}>{i18n.t("Stories")}</a><a href={i18n.href("/blog/")}>{i18n.t("Blog")}</a></div><span className="copyright">© {new Date().getFullYear()} TextHalo</span></footer>
    </div>
  );
}

export default Website;
