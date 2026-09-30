import { blogPosts, stories, type EditorialPage } from "./content";
import { BillingAccountPage, BillingSuccessPage, DesktopConnectPage, PasswordResetPage, PricingPage, SignInPage } from "./billing";
import Website from "./website";
import { DownloadPage } from "./DownloadPage";
import { DOWNLOAD_PAGE } from "./downloads";

const SITE_URL = "https://texthalo.app";
const SOURCE = "https://github.com/bharat2808/texthalo";

export function getSitePaths(): string[] {
  return [
    "/",
    DOWNLOAD_PAGE,
    "/pricing/",
    "/privacy/",
    "/sign-in/",
    "/desktop-connect/",
    "/reset-password/",
    "/account/billing/",
    "/account/billing/success/",
    "/demo/",
    "/blog/",
    ...blogPosts.map((page) => `/blog/${page.slug}/`),
    "/stories/",
    ...stories.map((page) => `/stories/${page.slug}/`),
  ];
}

function VideoWatchPage() {
  return <div className="site-shell"><EditorialHeader /><main className="editorial-main watch-main">
    <nav className="breadcrumbs" aria-label="Breadcrumb"><a href="/">Home</a><span>/</span><span>Product demo</span></nav>
    <div className="eyebrow"><span className="eyebrow-line" /> TEXTHALO FOR MAC</div>
    <h1>See how TextHalo reads selected text aloud</h1>
    <p className="editorial-intro">Watch the 15-second product tour to see TextHalo’s menu bar workflow, keyboard shortcut, and floating playback controls.</p>
    <figure className="watch-player">
      <video controls playsInline preload="metadata" poster="/videos/texthalo-product-demo-poster.png" width="960" height="540" aria-label="15-second TextHalo product demo with narration">
        <source src="/videos/texthalo-product-demo.mp4" type="video/mp4" />
        Your browser does not support embedded video.
      </video>
      <figcaption>TextHalo product tour · 15 seconds · Narrated with Kokoro</figcaption>
    </figure>
    <section className="watch-details">
      <h2>A small Mac app for listening while you work</h2>
      <p>TextHalo reads a passage you select in another app. Start playback with a keyboard shortcut, keep the floating player nearby, and choose Apple system voices or local AI speech engines.</p>
      <p>Speech synthesis runs locally on your Mac. Kokoro and Chatterbox model files are downloaded when you choose to use those engines.</p>
      <a className="button button-dark" href={DOWNLOAD_PAGE}>Download TextHalo for Mac <span aria-hidden="true">→</span></a>
    </section>
  </main><EditorialFooter /></div>;
}

function Mark() {
  return <span className="brand-mark" aria-hidden="true"><span /></span>;
}

function EditorialHeader() {
  return <>
    <div className="announcement"><span className="announcement-dot" /> TextHalo is open source <span className="announcement-separator">·</span> Made for macOS</div>
    <header className="site-header editorial-header">
      <a className="wordmark" href="/" aria-label="TextHalo home"><Mark /><span>TextHalo</span></a>
      <nav className="editorial-nav" aria-label="Main navigation">
        <a href="/#how-it-works">How it works</a>
        <a href="/#voices">Voices</a>
        <a href="/pricing/">Pricing</a>
        <a href="/demo/">Demo</a>
        <a href="/stories/">Stories</a>
        <a href="/blog/">Blog</a>
        <a href="/privacy/">Privacy</a>
        <a className="button button-dark nav-download" href={DOWNLOAD_PAGE}>Download for Mac <span aria-hidden="true">→</span></a>
      </nav>
    </header>
  </>;
}

function EditorialFooter() {
  return <footer className="site-footer"><a className="wordmark footer-wordmark" href="/"><Mark /><span>TextHalo</span></a><span className="footer-copy">A little more room to listen.</span><div className="footer-links"><a href="/privacy/">Privacy</a><a href="/blog/">Blog</a><a href="/stories/">Stories</a><a href={SOURCE}>GitHub</a><a href={`${SOURCE}/blob/master/LICENSE-APACHE`}>Apache 2.0</a></div><span className="copyright">© {new Date().getFullYear()} TextHalo</span></footer>;
}

function PrivacyPolicyPage() {
  return <div className="site-shell"><EditorialHeader /><main className="editorial-main article-main privacy-policy">
    <nav className="breadcrumbs" aria-label="Breadcrumb"><a href="/">Home</a><span>/</span><span>Privacy</span></nav>
    <article>
      <header className="article-header"><div className="eyebrow"><span className="eyebrow-line" /> YOUR WORDS, YOUR CHOICE</div><h1>Privacy policy</h1><p className="article-deck">This policy explains what TextHalo processes when you use the Mac app and related hosted services.</p><div className="article-byline"><span>Effective September 29, 2026</span></div></header>
      <div className="article-body">
        <h2>Speech on your Mac</h2>
        <p>When you use Apple system voices or a downloaded local speech model, TextHalo processes the selected text and generates speech on your Mac. TextHalo may download model files or app updates when you request them; those downloads are separate from sending text for speech.</p>
        <h2>Hosted speech</h2>
        <p>If you choose a hosted voice, the selected text is sent to TextHalo’s service and on to the speech provider so it can generate audio. The service streams the generated audio back to the app. Hosted speech requires an account and may require an eligible plan or credits. Do not send text to a hosted voice unless you are comfortable processing it this way.</p>
        <h2>Optional text enhancement</h2>
        <p>If you turn on semantic delivery-cue enhancement for hosted speech, the selected text is also sent to the configured enhancement service to prepare the speech request. You can turn this option off in the app’s voice settings.</p>
        <h2>Voice cloning</h2>
        <p>If you submit a recording to create a hosted voice clone, that recording and the details you provide are sent to TextHalo’s service and the voice-cloning provider for training and clone management. Only submit recordings you own or have permission to use. You can manage or delete your hosted clones from the app’s account area; deletion from TextHalo does not make claims about any independent copies or records retained by a provider.</p>
        <h2>Account and payments</h2>
        <p>If you create an account or use hosted features, TextHalo’s service processes account and service information needed to authenticate you, manage your hosted access, and maintain your credits or subscription. Payments and checkout are handled by the payment provider. TextHalo’s service receives payment-related identifiers and subscription or credit events needed to provide the service; payment card details are handled through checkout.</p>
        <h2>Local app data</h2>
        <p>App preferences, selected voice settings, and playback history are stored by the app on your Mac. Model files and audio used by the app may also be stored in local app data. Content submitted to hosted speech or hosted voice cloning is transmitted as described above.</p>
        <h2>Your choices and questions</h2>
        <p>You can choose local speech instead of hosted speech, disable optional text enhancement, and avoid hosted cloning. The app presents a separate acknowledgement before hosted speech can be used. For questions about this policy, contact the TextHalo maintainers through the project’s <a href={`${SOURCE}/issues`} target="_blank" rel="noreferrer">GitHub repository</a>.</p>
      </div>
    </article>
  </main><EditorialFooter /></div>;
}

function Card({ page, basePath }: { page: EditorialPage; basePath: string }) {
  return <article className="editorial-card">
    <div className="editorial-card-meta"><span>{page.category}</span><span>{page.readTime}</span></div>
    <h2><a href={`${basePath}${page.slug}/`}>{page.title}</a></h2>
    <p>{page.description}</p>
    <a className="text-link" href={`${basePath}${page.slug}/`}>Read story <span aria-hidden="true">→</span></a>
  </article>;
}

function EditorialIndex({ kind }: { kind: "blog" | "stories" }) {
  const isBlog = kind === "blog";
  const pages = isBlog ? blogPosts : stories;
  const title = isBlog ? "Notes on listening" : "Ways to make room to listen";
  const description = isBlog
    ? "Guides to Mac text to speech, local voice engines, privacy, and listening workflows from TextHalo."
    : "Practical ways to listen to articles and drafts on your Mac with TextHalo.";

  return <div className="site-shell"><EditorialHeader /><main className="editorial-main">
    <div className="eyebrow"><span className="eyebrow-line" /> {isBlog ? "THE TEXTHALO BLOG" : "PRODUCT WORKFLOWS"}</div>
    <h1>{title}</h1>
    <p className="editorial-intro">{description}</p>
    <div className="editorial-grid">{pages.map((page) => <Card key={page.slug} page={page} basePath={`/${kind}/`} />)}</div>
  </main><EditorialFooter /></div>;
}

function EditorialArticle({ page, kind }: { page: EditorialPage; kind: "blog" | "stories" }) {
  const basePath = `/${kind}/`;
  return <div className="site-shell"><EditorialHeader /><main className="editorial-main article-main">
    <nav className="breadcrumbs" aria-label="Breadcrumb"><a href="/">Home</a><span>/</span><a href={basePath}>{kind === "blog" ? "Blog" : "Stories"}</a></nav>
    <article>
      <header className="article-header">
        <div className="eyebrow"><span className="eyebrow-line" /> {page.category.toUpperCase()}</div>
        <h1>{page.title}</h1>
        <p className="article-deck">{page.description}</p>
        <div className="article-byline"><span>TextHalo team</span><span aria-hidden="true">·</span><time dateTime={page.datePublished}>{new Date(`${page.datePublished}T12:00:00Z`).toLocaleDateString("en", { year: "numeric", month: "long", day: "numeric", timeZone: "UTC" })}</time><span aria-hidden="true">·</span><span>{page.readTime}</span></div>
      </header>
      <div className="article-body">{page.blocks.map((block, index) => {
        if (block.type === "heading") return <h2 key={index}>{block.text}</h2>;
        if (block.type === "list") return <ul key={index}>{block.items.map((item) => <li key={item}>{item}</li>)}</ul>;
        return <p key={index}>{block.text}</p>;
      })}</div>
    </article>
    <aside className="article-cta"><div><strong>Listen to selected text on your Mac.</strong><span>TextHalo is free and open source.</span></div><a className="button button-dark" href={DOWNLOAD_PAGE}>Download TextHalo <span aria-hidden="true">→</span></a></aside>
    <a className="text-link back-link" href={basePath}>← Back to {kind}</a>
  </main><EditorialFooter /></div>;
}

export function SitePage({ pathname }: { pathname: string }) {
  if (pathname === DOWNLOAD_PAGE) return <DownloadPage />;
  if (pathname === "/") return <Website />;
  if (pathname === "/privacy/") return <PrivacyPolicyPage />;
  if (pathname === "/pricing/") return <PricingPage />;
  if (pathname === "/sign-in/") return <SignInPage />;
  if (pathname === "/desktop-connect/") return <DesktopConnectPage />;
  if (pathname === "/reset-password/") return <PasswordResetPage />;
  if (pathname === "/account/billing/") return <BillingAccountPage />;
  if (pathname === "/account/billing/success/") return <BillingSuccessPage />;
  if (pathname === "/demo/") return <VideoWatchPage />;
  if (pathname === "/blog/") return <EditorialIndex kind="blog" />;
  if (pathname === "/stories/") return <EditorialIndex kind="stories" />;
  const match = pathname.match(/^\/(blog|stories)\/([^/]+)\/$/);
  if (match) {
    const kind = match[1] as "blog" | "stories";
    const collection = kind === "blog" ? blogPosts : stories;
    const page = collection.find((item) => item.slug === match[2]);
    if (page) return <EditorialArticle page={page} kind={kind} />;
  }
  return <div className="site-shell"><EditorialHeader /><main className="editorial-main"><div className="eyebrow"><span className="eyebrow-line" /> PAGE NOT FOUND</div><h1>That page isn’t here.</h1><a className="text-link" href="/">Go to TextHalo home →</a></main><EditorialFooter /></div>;
}

export function normalizePath(pathname: string): string {
  if (pathname === "/") return "/";
  return `/${pathname.split("/").filter(Boolean).join("/")}/`;
}

export type SeoMetadata = {
  path: string;
  title: string;
  description: string;
  type: "website" | "article" | "video.other";
  published?: string;
  modified?: string;
  video?: {
    name: string;
    description: string;
    thumbnailUrl: string;
    contentUrl: string;
    duration: string;
    uploadDate: string;
  };
  schema: Record<string, unknown>;
};

const defaultMetadata = (path: string): SeoMetadata => ({
  path,
  title: "TextHalo for Mac — Local Text to Speech",
  description: "TextHalo is a free, open-source Mac app that reads selected text aloud with Apple, Kokoro, and Chatterbox voices—all synthesized locally on your Mac.",
  type: "website",
  schema: {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: "TextHalo",
    applicationCategory: "UtilitiesApplication",
    operatingSystem: "macOS",
    description: "A free, open-source Mac menu bar app for reading selected text aloud with local speech engines.",
    url: SITE_URL,
    downloadUrl: `${SITE_URL}${DOWNLOAD_PAGE}`,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    license: `${SOURCE}/blob/master/LICENSE-APACHE`,
  },
});

export function getSeoMetadata(pathname: string): SeoMetadata {
  const path = normalizePath(pathname);
  if (path === DOWNLOAD_PAGE) return { ...defaultMetadata(path), title: "Download TextHalo for Apple Silicon Mac", description: "Download TextHalo for your Apple Silicon Mac and start listening to selected text." };
  if (path === "/") return defaultMetadata(path);
  if (path === "/pricing/") {
    return { ...defaultMetadata(path), title: "TextHalo Plans — Local Voices and Hosted Speech", description: "Compare TextHalo Free, Plus, and Creator plans. Keep local Apple, Kokoro, and Chatterbox voices, then add hosted Fish Audio credits when you need them." };
  }
  if (path === "/privacy/") {
    const title = "Privacy Policy | TextHalo";
    const description = "How TextHalo handles local speech, hosted speech, optional text enhancement, voice cloning, account data, and payments.";
    return { ...defaultMetadata(path), title, description, schema: { "@context": "https://schema.org", "@type": "WebPage", name: title, description, url: `${SITE_URL}${path}` } };
  }
  if (path === "/sign-in/") {
    return { ...defaultMetadata(path), title: "Sign in to TextHalo", description: "Sign in or create a TextHalo account to continue to hosted voice checkout." };
  }
  if (path === "/desktop-connect/") {
    return { ...defaultMetadata(path), title: "Connect TextHalo Desktop", description: "Sign in securely to connect your TextHalo Mac app." };
  }
  if (path === "/reset-password/") {
    return { ...defaultMetadata(path), title: "Reset your password | TextHalo", description: "Choose a new password for your TextHalo account." };
  }
  if (path === "/account/billing/") {
    return { ...defaultMetadata(path), title: "Credits & billing | TextHalo", description: "View your TextHalo plan, monthly and top-up credits, and billing settings." };
  }
  if (path === "/account/billing/success/") {
    return { ...defaultMetadata(path), title: "Checkout complete | TextHalo", description: "Return to TextHalo after checking out for hosted speech credits." };
  }
  if (path === "/demo/") {
    const title = "TextHalo Mac App Demo — 15-Second Product Tour";
    const description = "Watch TextHalo read selected text aloud on Mac. This 15-second product tour shows the menu bar app, shortcut, and floating playback controls.";
    const video = {
      name: "TextHalo for Mac: 15-Second Product Tour",
      description,
      thumbnailUrl: `${SITE_URL}/videos/texthalo-product-demo-poster.png`,
      contentUrl: `${SITE_URL}/videos/texthalo-product-demo.mp4`,
      duration: "PT15S",
      uploadDate: "2026-09-25",
    };
    const videoId = `${SITE_URL}${path}#video`;
    return {
      ...defaultMetadata(path),
      title,
      description,
      type: "video.other",
      modified: "2026-09-27",
      video,
      schema: {
        "@context": "https://schema.org",
        "@graph": [
          { "@type": "WebPage", name: title, description, "url": `${SITE_URL}${path}`, "mainEntity": { "@id": videoId } },
          { "@type": "VideoObject", "@id": videoId, ...video, "mainEntityOfPage": `${SITE_URL}${path}` },
        ],
      },
    };
  }
  const isBlog = path === "/blog/" || path.startsWith("/blog/");
  const kind = isBlog ? "blog" : "stories";
  const collection = isBlog ? blogPosts : stories;
  if (path === `/${kind}/`) {
    const title = isBlog ? "Mac Text to Speech Guides & Tips | TextHalo Blog" : "Mac Listening Workflows with TextHalo";
    const description = isBlog
    ? "Practical guides to Mac text to speech, local voice engines, privacy, and listening workflows from TextHalo."
      : "Explore practical ways to listen to articles and drafts on your Mac with TextHalo.";
    return { ...defaultMetadata(path), title, description, schema: { "@context": "https://schema.org", "@type": "CollectionPage", name: title, description, url: `${SITE_URL}${path}` } };
  }
  const match = path.match(/^\/(blog|stories)\/([^/]+)\/$/);
  const page = match && collection.find((item) => item.slug === match[2]);
  if (!page) return { ...defaultMetadata(path), title: "Page not found | TextHalo", description: "This TextHalo page could not be found." };
  const title = `${page.title} | TextHalo`;
  const image = `${SITE_URL}/videos/texthalo-product-demo-poster.png`;
  const schema = {
    "@context": "https://schema.org",
    "@type": isBlog ? "BlogPosting" : "Article",
    headline: page.title,
    description: page.description,
    datePublished: page.datePublished,
    dateModified: page.dateModified ?? page.datePublished,
    author: { "@type": "Organization", name: "TextHalo", url: SITE_URL },
    publisher: { "@type": "Organization", name: "TextHalo", url: SITE_URL },
    mainEntityOfPage: `${SITE_URL}${path}`,
    image,
  };
  return { ...defaultMetadata(path), title, description: page.description, type: "article", published: page.datePublished, modified: page.dateModified ?? page.datePublished, schema };
}
