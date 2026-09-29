import { SiteHeader } from "./billing";

const RELEASES = "https://github.com/bharat2808/texthalo/releases/latest";
const DOWNLOAD = `${RELEASES}/download/TextHalo-macOS-aarch64.dmg`;
const SOURCE = "https://github.com/bharat2808/texthalo";

function Mark({ small = false }: { small?: boolean }) {
  return (
    <span className={`brand-mark${small ? " brand-mark-small" : ""}`} aria-hidden="true">
      <span />
    </span>
  );
}

function Arrow({ diagonal = false }: { diagonal?: boolean }) {
  return <span aria-hidden="true" className="arrow">{diagonal ? "↗" : "→"}</span>;
}

function SoundBars() {
  return (
    <div className="sound-bars" aria-hidden="true">
      {[18, 31, 23, 43, 29, 55, 36, 48, 25, 39, 56, 28, 44, 21, 34, 49, 27, 40, 19, 33, 51, 24, 38, 17, 31, 45, 22, 37, 16].map((height, index) => (
        <i key={index} style={{ height }} />
      ))}
    </div>
  );
}

function ProductVideo() {
  return (
    <div className="product-video">
      <div className="product-video-label"><span className="live-dot" /> SEE TEXTHALO IN ACTION</div>
      <video
        controls
        playsInline
        preload="metadata"
        poster="/videos/texthalo-product-demo-poster.png"
        aria-label="TextHalo product demo with narration"
      >
        <source src="/videos/texthalo-product-demo.mp4" type="video/mp4" />
        Your browser does not support embedded video.
      </video>
      <div className="product-video-caption"><span>15 SECOND PRODUCT TOUR</span><span>VOICEOVER · KOKORO</span></div>
      <a className="video-watch-link" href="/demo/">Watch the product tour <Arrow /></a>
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
            <div className="eyebrow"><span className="eyebrow-line" /> YOUR WORDS, IN A DIFFERENT WAY</div>
            <h1>A little more<br /><em>room to listen.</em></h1>
            <p className="hero-text">Select a passage. Press a shortcut. Let your Mac read while you think, stretch, or look away for a while.</p>
            <div className="hero-actions">
              <a className="button button-dark button-large" href={DOWNLOAD} target="_blank" rel="noreferrer">Download for Mac <Arrow /></a>
              <a className="text-link" href="#how-it-works">See how it works <Arrow /></a>
            </div>
            <div className="hero-meta"><span><i className="apple-mark">●</i> Built for macOS</span><span className="meta-divider" /><span>Free &amp; open source</span></div>
          </div>
          <ProductVideo />
          <div className="hero-footnote"><span>01 / 03</span><span className="footnote-line" /><span>MAKE SPACE FOR A LISTEN</span></div>
        </section>

        <section className="ticker" aria-label="TextHalo benefits">
          <div>YOUR PACE <b>✳</b> YOUR PLACE <b>✳</b> YOUR VOICE <b>✳</b> YOUR PACE <b>✳</b> YOUR PLACE <b>✳</b> YOUR VOICE <b>✳</b></div>
        </section>

        <section id="how-it-works" className="how section-wrap">
          <div className="section-heading"><div><div className="eyebrow"><span className="eyebrow-line" /> SIMPLE BY DESIGN</div><h2>From selected to spoken<br /><em>in a single breath.</em></h2></div><p>TextHalo lives in your menu bar. It stays out of the way until you want to listen.</p></div>
          <div className="steps-grid">
            <article className="step-card"><div className="step-top"><span>01</span><span className="step-icon selection-icon">⌖</span></div><h3>Find your words</h3><p>Select text in the app you’re already using. An article, a draft, a long email — pick up wherever you are.</p><div className="mini-selection">“Pick up wherever you are.” <i /></div></article>
            <article className="step-card"><div className="step-top"><span>02</span><span className="step-icon shortcut-icon">⌘<small>⇧ S</small></span></div><h3>Press your shortcut</h3><p>A quick keyboard shortcut starts reading. Your selection stays right where it is.</p><div className="mini-shortcut"><span>Read selection</span><kbd>⌘</kbd><kbd>⇧</kbd><kbd>S</kbd></div></article>
            <article className="step-card step-card-accent"><div className="step-top"><span>03</span><span className="step-icon sound-icon">≈</span></div><h3>Settle in and listen</h3><p>Follow along in the floating player. Stop or change your voice whenever you like.</p><div className="mini-player"><div className="mini-play">▶</div><SoundBars /></div></article>
          </div>
        </section>

        <section id="voices" className="voices-section">
          <div className="voices section-wrap">
            <div className="voices-copy"><div className="eyebrow eyebrow-light"><span className="eyebrow-line" /> A VOICE THAT FITS THE MOMENT</div><h2>Three ways to<br /><em>hear it.</em></h2><p>Start with a voice already on your Mac. When you want more, choose a local AI engine and find the sound that suits you.</p><a className="light-link" href={SOURCE} target="_blank" rel="noreferrer">See all supported voices <Arrow /></a></div>
            <div className="engine-list">
              <article className="engine-card engine-apple"><div className="engine-number">01</div><div className="engine-main"><div className="engine-icon system-icon">◖</div><div><h3>Apple voices</h3><p>Ready when you are. Uses voices installed on your Mac.</p></div></div><span className="engine-tag">READY TO GO</span></article>
              <article className="engine-card"><div className="engine-number">02</div><div className="engine-main"><div className="engine-icon kokoro-icon">k</div><div><h3>Kokoro</h3><p>Lightweight, natural-sounding speech that runs locally.</p></div></div><span className="engine-tag">LOCAL AI</span></article>
              <article className="engine-card"><div className="engine-number">03</div><div className="engine-main"><div className="engine-icon chatter-icon">✳</div><div><h3>Chatterbox</h3><p>Expressive multilingual speech with your own reference voice.</p></div></div><span className="engine-tag">LOCAL AI</span></article>
              <p className="engine-footnote"><span>↳</span> AI models download on your Mac when you choose to use them.</p>
            </div>
          </div>
          <div className="dark-orbit dark-orbit-a" /><div className="dark-orbit dark-orbit-b" />
        </section>

        <section id="privacy" className="privacy section-wrap">
          <div className="privacy-art"><div className="privacy-ring ring-a"/><div className="privacy-ring ring-b"/><div className="privacy-center"><Mark /><span>ON YOUR MAC</span></div><div className="privacy-pill pill-top">LOCAL SPEECH</div><div className="privacy-pill pill-bottom">LOCAL OR HOSTED</div></div>
          <div className="privacy-copy"><div className="eyebrow"><span className="eyebrow-line" /> YOUR WORDS, YOUR CHOICE</div><h2>Privacy by<br /><em>where it happens.</em></h2><p>Apple, Kokoro, and Chatterbox speech runs on your Mac. If you choose hosted speech, selected text is sent to TextHalo’s service and the speech provider to generate audio. Optional text enhancement and hosted voice cloning involve additional processing.</p><div className="privacy-points"><div><span className="check-mark">✓</span><span><strong>Choose local or hosted speech</strong><small>Local voices process text on your Mac; hosted voices process it remotely.</small></span></div><div><span className="check-mark">✓</span><span><strong>Optional features stay optional</strong><small>Text enhancement and voice cloning are used only when you choose them.</small></span></div></div><a className="text-link" href="/privacy/">Read the privacy policy <Arrow /></a></div>
        </section>

        <section className="closing-cta"><div className="closing-inner"><div className="closing-mark"><Mark /></div><div className="eyebrow eyebrow-light"><span className="eyebrow-line" /> READY WHEN YOU ARE</div><h2>Give your eyes<br /><em>a little break.</em></h2><p>TextHalo is free, open source, and made for your Mac.</p><a className="button button-cream button-large" href={DOWNLOAD} target="_blank" rel="noreferrer">Get TextHalo for macOS <Arrow /></a><span className="cta-version">Current version 0.1.6 <span>·</span> Requires macOS</span></div><div className="cta-decoration cta-dec-a"/><div className="cta-decoration cta-dec-b"/></section>
      </main>

      <footer className="site-footer"><a className="wordmark footer-wordmark" href="#top"><Mark /><span>TextHalo</span></a><span className="footer-copy">A little more room to listen.</span><div className="footer-links"><a href="/pricing/">Pricing</a><a href="/demo/">Demo</a><a href="/stories/">Stories</a><a href="/blog/">Blog</a><a href={SOURCE} target="_blank" rel="noreferrer">GitHub <Arrow diagonal /></a><a href={`${SOURCE}/blob/master/LICENSE-APACHE`} target="_blank" rel="noreferrer">Apache 2.0</a><a href={`${SOURCE}/issues/new`} target="_blank" rel="noreferrer">Feedback <Arrow diagonal /></a></div><span className="copyright">© {new Date().getFullYear()} TextHalo</span></footer>
    </div>
  );
}

export default Website;
