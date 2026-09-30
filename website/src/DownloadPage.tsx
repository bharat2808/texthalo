import { SiteHeader } from "./billing";
import { MAC_DOWNLOAD } from "./downloads";

export function DownloadPage() {
  return <div className="site-shell"><SiteHeader /><main className="editorial-main download-main">
    <div className="eyebrow">TEXTHALO FOR MAC</div>
    <h1>A voice for your Mac.</h1>
    <p className="editorial-intro">TextHalo requires a Mac with Apple Silicon (an Apple M-series chip). Intel Macs are not supported.</p>
    <div className="download-options">
      <section className="download-card">
        <h2>Apple Silicon</h2>
        <p>Check your chip in Apple menu → About This Mac.</p>
        <a className="button button-dark" href={MAC_DOWNLOAD}>Download for Apple Silicon <span aria-hidden="true">↓</span></a>
      </section>
    </div>
    <p className="download-help">Open the downloaded DMG, drag TextHalo into Applications, then launch it from Applications.</p>
    <a className="text-link" href="https://github.com/bharat2808/texthalo/releases">Release notes and previous versions →</a>
  </main></div>;
}
