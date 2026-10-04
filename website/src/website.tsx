import { DEMO_VIDEO } from "./demo";
import { SiteHeader } from "./billing";
import { MAC_DOWNLOAD as DOWNLOAD } from "./downloads";
import { version } from "../../package.json";


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
  return (
    <div className="product-video">
      <div className="product-video-label"><span className="live-dot" /> SEE TEXTHALO IN ACTION</div>
      <video
        controls
        playsInline
        preload="metadata"
        poster={DEMO_VIDEO.poster}
        aria-label="30-second animated TextHalo demo: Listen, Dictate, and Assistant"
      >
        <source src={DEMO_VIDEO.src} type="video/mp4" />
        <track kind="captions" src={DEMO_VIDEO.captions} srcLang="en" label="English" />
        Your browser does not support embedded video.
      </video>
      <div className="product-video-caption"><span>30 SECOND WORKFLOW DEMO</span><span>VOICEOVER · FISH AUDIO</span></div>
      <a className="video-watch-link" href="/demo/">Watch the product demo <Arrow /></a>
    </div>
  );
}

function Website() {
  return (
    <div className="site-shell">
      <SiteHeader />

      <main id="top">
        <section className="hero section-wrap">
          <div className="hero-copy">
            <div className="eyebrow"><span className="eyebrow-line" /> LISTEN. DICTATE. GET A LITTLE HELP.</div>
            <h1>Your words.<br /><em>More possibilities.</em></h1>
            <p className="hero-text">Listen to selected text, turn speech into writing, or ask Assistant to write, edit, and read aloud. All from a shortcut on your Mac.</p>
            <div className="hero-actions">
              <a className="button button-dark button-large" href={DOWNLOAD} target="_blank" rel="noreferrer">Download for Mac <Arrow /></a>
              <a className="text-link" href="#how-it-works">See how it works <Arrow /></a>
            </div>
            <div className="hero-meta"><span><i className="apple-mark">●</i> Requires Apple Silicon</span><span className="meta-divider" /><span>Free local features</span></div>
          </div>
          <ProductVideo />
          <div className="hero-footnote"><span>01 / 03</span><span className="footnote-line" /><span>ONE MAC APP, THREE WAYS TO WORK</span></div>
        </section>

        <section className="ticker" aria-label="TextHalo benefits">
          <div>LISTEN <b>✳</b> DICTATE <b>✳</b> ASSISTANT <b>✳</b> LISTEN <b>✳</b> DICTATE <b>✳</b> ASSISTANT <b>✳</b></div>
        </section>

        <section id="how-it-works" className="how section-wrap">
          <div className="section-heading"><div><div className="eyebrow"><span className="eyebrow-line" /> THREE WAYS TO WORK</div><h2>Read less. Say more.<br /><em>Keep your flow.</em></h2></div><p>TextHalo lives in your menu bar. Choose your shortcuts and work in the app you already have open.</p></div>
          <div className="steps-grid">
            <article className="step-card"><div className="step-top"><span>01 / LISTEN</span><span className="step-icon sound-icon">≈</span></div><h3>Hear your words</h3><p>Select an article, a draft, or an email. Press your shortcut to hear it aloud, with a floating player to stop playback.</p><div className="mini-shortcut"><span>Read selection</span><kbd>⌘</kbd><kbd>⇧</kbd><kbd>S</kbd></div></article>
            <article className="step-card"><div className="step-top"><span>02 / DICTATE</span><span className="step-icon selection-icon">⌖</span></div><h3>Speak instead of typing</h3><p>Hold your dictation shortcut, speak, then release. Insert your words into the current app or copy them to the clipboard.</p><div className="mini-selection">“Here’s what I have in mind.” <i /></div></article>
            <article className="step-card step-card-accent"><div className="step-top"><span>03 / ASSISTANT</span><span className="step-icon chatter-icon">✳</span></div><h3>Ask for a little help</h3><p>Hold your Assistant shortcut to ask for writing, an edit, or something read aloud. Use selected text and available window context.</p><div className="mini-selection">“Make this paragraph clearer.” <i /></div></article>
          </div>
          <p className="workflow-note">Listen starts with ⌘⇧S. Set up your own Dictate and Assistant shortcuts in the app.</p>
        </section>

        <section id="voices" className="voices-section">
          <div className="voices section-wrap">
            <div className="voices-copy"><div className="eyebrow eyebrow-light"><span className="eyebrow-line" /> A VOICE THAT FITS THE MOMENT</div><h2>Four ways to<br /><em>hear it.</em></h2><p>Start with Apple voices, download a local AI engine, or sign in for Fish Audio hosted voices. Preview a voice before you choose it.</p></div>
            <div className="engine-list">
              <article className="engine-card engine-apple"><div className="engine-number">01</div><div className="engine-main"><div className="engine-icon system-icon">◖</div><div><h3>Apple voices</h3><p>Ready when you are. Uses voices installed on your Mac.</p></div></div><span className="engine-tag">READY TO GO</span></article>
              <article className="engine-card"><div className="engine-number">02</div><div className="engine-main"><div className="engine-icon kokoro-icon">k</div><div><h3>Kokoro</h3><p>Lightweight, natural-sounding speech that runs locally.</p></div></div><span className="engine-tag">LOCAL AI</span></article>
              <article className="engine-card"><div className="engine-number">03</div><div className="engine-main"><div className="engine-icon chatter-icon">✳</div><div><h3>Chatterbox</h3><p>Expressive multilingual speech with your own reference voice.</p></div></div><span className="engine-tag">LOCAL AI</span></article>
              <article className="engine-card"><div className="engine-number">04</div><div className="engine-main"><div className="engine-icon system-icon">≈</div><div><h3>Fish Audio</h3><p>Stream hosted voices and create saved voice clones with eligible access.</p></div></div><span className="engine-tag">HOSTED · CREDITS</span></article>
              <p className="engine-footnote"><span>↳</span> Kokoro and Chatterbox need model downloads. Fish Audio needs sign-in, credits, and your hosted-text acknowledgement.</p>
            </div>
          </div>
          <div className="dark-orbit dark-orbit-a" /><div className="dark-orbit dark-orbit-b" />
        </section>

        <section id="privacy" className="privacy section-wrap">
          <div className="privacy-art"><div className="privacy-ring ring-a"/><div className="privacy-ring ring-b"/><div className="privacy-center"><Mark /><span>ON YOUR MAC</span></div><div className="privacy-pill pill-top">LOCAL SPEECH</div><div className="privacy-pill pill-bottom">LOCAL OR HOSTED</div></div>
          <div className="privacy-copy"><div className="eyebrow"><span className="eyebrow-line" /> YOUR WORDS, YOUR CHOICE</div><h2>Privacy by<br /><em>where it happens.</em></h2><p>Local voices and dictation process on your Mac. On-device Assistant uses Apple Intelligence when available. Cloud Assistant sends your request and text context to TextHalo and its model provider; Fish Audio separately receives the words you ask it to speak.</p><div className="privacy-points"><div><span className="check-mark">✓</span><span><strong>Choose where processing happens</strong><small>Use local speech and dictation, or enable hosted services with their privacy notices.</small></span></div><div><span className="check-mark">✓</span><span><strong>Optional features stay optional</strong><small>Cloud Assistant requires consent. Assistant screenshots and microphone audio are not uploaded for cloud generation.</small></span></div></div><a className="text-link" href="/privacy/">Read the privacy policy <Arrow /></a></div>
        </section>

        <section className="closing-cta"><div className="closing-inner"><div className="closing-mark"><Mark /></div><div className="eyebrow eyebrow-light"><span className="eyebrow-line" /> READY WHEN YOU ARE</div><h2>Listen. Dictate.<br /><em>Get a little help.</em></h2><p>Free local features. Open-source app. Optional paid Cloud Assistant and hosted voices.</p><a className="button button-cream button-large" href={DOWNLOAD} target="_blank" rel="noreferrer">Get TextHalo for macOS <Arrow /></a><span className="cta-version">Current version {version} <span>·</span> Requires Apple Silicon</span></div><div className="cta-decoration cta-dec-a"/><div className="cta-decoration cta-dec-b"/></section>
      </main>

      <footer className="site-footer"><a className="wordmark footer-wordmark" href="#top"><Mark /><span>TextHalo</span></a><span className="footer-copy">Listen. Dictate. Get a little help.</span><div className="footer-links"><a href="/pricing/">Pricing</a><a href="/demo/">Demo</a><a href="/stories/">Stories</a><a href="/blog/">Blog</a><a href="https://github.com/bharat2808/texthalo" target="_blank" rel="noreferrer">GitHub <span className="arrow" aria-hidden="true">↗</span></a></div><span className="copyright">© {new Date().getFullYear()} TextHalo</span></footer>
    </div>
  );
}

export default Website;
