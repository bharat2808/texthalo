import { formatPlanFeature, formatWebsitePlanPrice } from "./localization";
import { LanguagePicker, useWebsiteI18n } from "./i18n";
import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { MAC_DOWNLOAD as DOWNLOAD } from "./downloads";
import { authClient, billingApiHeaders, billingApiUrl, getAccessToken, promptSignIn, safeInternalReturnTo, startCheckout, startCreditTopupCheckout } from "./auth";

type PlanId = "plus" | "creator";
type PlanCard = {
  id: "free" | PlanId;
  name: string;
  description: string;
  price: { unitAmount: number; currency: string; interval: string | null; intervalCount?: number } | null;
  creditsPerPeriod: number;
  cloneLimit: number;
  features: string[];
  displayOrder: number;
  action?: string;
};
type CreditTopupPack = { id: string; priceCents: number; credits: number };
type CreditBalanceBreakdown = { totalCredits: number; planCredits: number; topupCredits: number; otherCredits: number; debtCredits?: number };
type BillingStatus = { billingEnabled: boolean; plans: PlanCard[]; topups?: CreditTopupPack[] };
type BillingAccount = {
  availableCredits?: number;
  creditBreakdown?: CreditBalanceBreakdown;
  subscription?: { planId: string; status: string; currentPeriodEnd: string; cancelAtPeriodEnd: boolean } | null;
};

// Keep public pricing visible even if the billing API is temporarily offline.
// The backend remains the authority when checkout starts.
const PUBLIC_TOPUP_PACKS: CreditTopupPack[] = [
  { id: "topup-5", priceCents: 500, credits: 30_000 },
  { id: "topup-10", priceCents: 1_000, credits: 60_000 },
  { id: "topup-20", priceCents: 2_000, credits: 120_000 },
];

const FREE_FEATURES = [
  "Apple native voices",
  "Kokoro local AI voices",
  "Chatterbox with built-in local voice cloning",
  "Speech generated on your Mac",
  "On-device dictation with macOS or Whisper",
  "Apple Intelligence Assistant where available",
];

const FALLBACK_PLANS: PlanCard[] = [
  {
    id: "free" as const,
    name: "Free",
    price: { unitAmount: 0, currency: "usd", interval: null },
    creditsPerPeriod: 0,
    cloneLimit: 0,
    displayOrder: 0,
    description: "Listen and dictate on your Mac.",
    features: FREE_FEATURES,
    action: "Download TextHalo",
  },
  {
    id: "plus" as const,
    name: "Plus",
    price: { unitAmount: 999, currency: "usd", interval: "month", intervalCount: 1 },
    creditsPerPeriod: 80_000,
    cloneLimit: 0,
    displayOrder: 1,
    description: "Hosted voices when you want a faster path.",
    features: [
      "Everything in Free",
      "Fish Audio hosted voices",
      "80,000 hosted credits each month",
      "Saved Fish Audio clones (live limit shown at checkout)",
    ],
    action: "Choose Plus",
  },
  {
    id: "creator" as const,
    name: "Creator",
    price: { unitAmount: 1999, currency: "usd", interval: "month", intervalCount: 1 },
    creditsPerPeriod: 150_000,
    cloneLimit: 0,
    displayOrder: 2,
    description: "More hosted audio, with saved Fish clones.",
    features: [
      "Everything in Free",
      "Fish Audio hosted voices",
      "150,000 hosted credits each month",
      "More saved Fish Audio clones (live limit shown at checkout)",
    ],
    action: "Choose Creator",
  },
];

const TURNSTILE_SITE_KEY = import.meta.env.VITE_TURNSTILE_SITE_KEY?.trim() ?? "";

declare global {
  interface Window {
    turnstile?: {
      render: (container: HTMLElement, options: { sitekey: string; action: string; callback: (token: string) => void; "error-callback": () => void; "expired-callback": () => void }) => string;
      remove: (widgetId: string) => void;
    };
  }
}

function Mark() {
  return <span className="brand-mark" aria-hidden="true"><span /></span>;
}

export function SiteHeader() {
  const i18n = useWebsiteI18n();
  const [menuOpen, setMenuOpen] = useState(false);
  const accountMenuRef = useRef<HTMLDetailsElement>(null);
  const session = authClient.useSession();

  useEffect(() => {
    function closeAccountMenu(event: PointerEvent) {
      const menu = accountMenuRef.current;
      if (menu?.open && event.target instanceof Node && !menu.contains(event.target)) menu.open = false;
    }
    function closeAccountMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape" && accountMenuRef.current?.open) {
        accountMenuRef.current.open = false;
        accountMenuRef.current.querySelector("summary")?.focus();
      }
    }
    document.addEventListener("pointerdown", closeAccountMenu);
    document.addEventListener("keydown", closeAccountMenuOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeAccountMenu);
      document.removeEventListener("keydown", closeAccountMenuOnEscape);
    };
  }, []);

  async function signOutFromHeader() {
    try {
      await authClient.signOut();
    } catch {
      // Redirect even if an already-expired session cannot be cleared remotely.
    } finally {
      window.location.assign(i18n.href("/pricing/"));
    }
  }

  return <>
    <div className="announcement"><span className="announcement-dot" /> {i18n.t("Free local features")} <span className="announcement-separator">·</span> {i18n.t("Requires Apple Silicon")} </div>
    <header className="site-header billing-header">
      <a className="wordmark" href={i18n.href("/")}><Mark /><span>TextHalo</span></a>
      <nav className={menuOpen ? "nav-open" : ""} aria-label={i18n.t("Main navigation")}>
        <a href={i18n.href("/#how-it-works")} onClick={() => setMenuOpen(false)}>{i18n.t("How it works")}</a>
        <a href={i18n.href("/privacy/")} onClick={() => setMenuOpen(false)}>{i18n.t("Privacy")}</a>
        <a href={i18n.href("/pricing/")} onClick={() => setMenuOpen(false)}>{i18n.t("Pricing")}</a>
        <a href={i18n.href("/account/billing/")} onClick={() => setMenuOpen(false)}>{i18n.t("Billing")}</a>
        <a href={i18n.href("/demo/")} onClick={() => setMenuOpen(false)}>{i18n.t("Demo")}</a>
        <a href={i18n.href("/stories/")} onClick={() => setMenuOpen(false)}>{i18n.t("Stories")}</a>
        <a href={i18n.href("/blog/")} onClick={() => setMenuOpen(false)}>{i18n.t("Blog")}</a>
        <a className="mobile-signin" href={i18n.href("/sign-in/")}>{i18n.t("Sign in")}</a>
        <a className="button button-dark nav-download" href={i18n.href(DOWNLOAD)} target="_blank" rel="noreferrer">{i18n.t("Get TextHalo")} <span className="arrow">→</span></a>
      </nav>
      <div className="header-account-tools"><LanguagePicker />
        {session.data?.user ? <details ref={accountMenuRef} className="header-account">
          <summary aria-label={i18n.t("Signed in as {0}", [session.data.user.email])} title={session.data.user.email}>
            <span className="header-account-avatar" aria-hidden="true">{session.data.user.email.slice(0, 1).toUpperCase()}</span><span className="header-account-chevron" aria-hidden="true">⌄</span>
          </summary>
          <div className="header-account-menu">
            <span className="header-account-label">{i18n.t("SIGNED IN AS")}</span>
            <strong>{session.data.user.email}</strong>
            <a href={i18n.href("/account/billing/")}>{i18n.t("Credits & billing")} <span className="arrow">→</span></a>
            <button type="button" onClick={() => void signOutFromHeader()}>{i18n.t("Sign out")}</button>
          </div>
        </details> : <a className="header-signin" href={i18n.href("/sign-in/")}>{i18n.t("Sign in")} <span className="arrow">→</span></a>}
        <button className="menu-toggle" aria-label={menuOpen ? i18n.t("Close menu") : i18n.t("Open menu")} aria-expanded={menuOpen} onClick={() => setMenuOpen(!menuOpen)}>{menuOpen ? "×" : "☰"}</button>
      </div>
    </header>
  </>;
}

function isLocalSignup() {
  return typeof window !== "undefined" && ["localhost", "127.0.0.1", "::1"].includes(window.location.hostname);
}

function TurnstileWidget({ active, onToken }: { active: boolean; onToken: (token: string) => void }) {
  const i18n = useWebsiteI18n();
  const container = useRef<HTMLDivElement>(null);
  const onTokenRef = useRef(onToken);
  onTokenRef.current = onToken;
  useEffect(() => {
    if (!active || !TURNSTILE_SITE_KEY || isLocalSignup() || !container.current) return;
    let widgetId: string | undefined;
    let cancelled = false;
    const render = () => {
      if (cancelled || !container.current || !window.turnstile) return;
      widgetId = window.turnstile.render(container.current, {
        sitekey: TURNSTILE_SITE_KEY,
        action: "signup",
        callback: (token) => onTokenRef.current(token),
        "error-callback": () => onTokenRef.current(""),
        "expired-callback": () => onTokenRef.current(""),
      });
    };
    if (window.turnstile) render();
    else {
      let script = document.querySelector<HTMLScriptElement>("script[data-texthalo-turnstile]");
      if (!script) {
        script = document.createElement("script");
        script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
        script.async = true;
        script.defer = true;
        script.dataset.texthaloTurnstile = "true";
        document.head.appendChild(script);
      }
      script.addEventListener("load", render, { once: true });
    }
    return () => {
      cancelled = true;
      if (widgetId && window.turnstile) window.turnstile.remove(widgetId);
    };
  }, [active]);
  if (!active || isLocalSignup()) return null;
  return TURNSTILE_SITE_KEY ? <div className="turnstile-widget" data-action="turnstile-spin-v1" ref={container} /> : <p className="signin-config-note">{i18n.t("Signup verification isn’t configured on this website.")}</p>;
}

function BillingFooter() {
  const i18n = useWebsiteI18n();
  return <footer className="site-footer"><a className="wordmark footer-wordmark" href={i18n.href("/")}><Mark /><span>TextHalo</span></a><span className="footer-copy">{i18n.t("Listen. Dictate. Get a little help.")}</span><div className="footer-links"><a href={i18n.href("/")}>{i18n.t("Home")}</a><a href={i18n.href("/demo/")}>{i18n.t("Demo")}</a></div><span className="copyright">© {new Date().getFullYear()} TextHalo</span></footer>;
}

export function createDesktopHandoffAttempt() {
  let started = false;
  let inFlight = false;
  let completed = false;
  return {
    async run(connect: () => Promise<void>, retry = false) {
      if (inFlight || completed || (started && !retry)) return;
      started = true;
      inFlight = true;
      try {
        await connect();
        completed = true;
      } finally {
        inFlight = false;
      }
    },
  };
}

export function DesktopConnectPage() {
  const i18n = useWebsiteI18n();
  const session = authClient.useSession();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [attempt] = useState(createDesktopHandoffAttempt);
  const [browserReady, setBrowserReady] = useState(false);
  useEffect(() => setBrowserReady(true), []);
  const params = new URLSearchParams(browserReady ? window.location.search : "");
  const redirectUri = params.get("redirect_uri") ?? "";
  const state = params.get("state") ?? "";
  const codeChallenge = params.get("code_challenge") ?? "";
  const validRequest = (() => {
    try {
      const uri = new URL(redirectUri);
      return uri.protocol === "http:" && ["127.0.0.1", "localhost"].includes(uri.hostname) && Boolean(uri.port) && uri.pathname === "/desktop-auth-callback" && !uri.search && !uri.hash && /^[A-Za-z0-9_-]{32,128}$/.test(state) && /^[A-Za-z0-9_-]{43}$/.test(codeChallenge);
    } catch { return false; }
  })();
  const returnTo = browserReady ? `${window.location.pathname}${window.location.search}` : "/desktop-connect/";

  const accountEmail = session.data?.user?.email;
  const continueToApp = useCallback(async (retry = false) => {
    try {
      await attempt.run(async () => {
        setBusy(true); setError("");
        if (!validRequest) throw new Error(i18n.t("This sign-in link is invalid or has expired. Return to TextHalo and start again."));
        if (!billingApiUrl) throw new Error(i18n.t("The TextHalo service is not configured on this website."));
        const token = await getAccessToken();
        if (!token) throw new Error(i18n.t("Your browser sign-in has expired. Please sign in again."));
        const response = await fetch(`${billingApiUrl}/v1/desktop/handoffs`, {
          method: "POST",
          headers: billingApiHeaders({ Authorization: `Bearer ${token}`, "Content-Type": "application/json" }),
          body: JSON.stringify({ codeChallenge, state, redirectUri, accountEmail }),
          signal: AbortSignal.timeout(15_000),
        });
        const result = await response.json().catch(() => null) as { callbackUrl?: string; error?: string } | null;
        if (!response.ok || !result?.callbackUrl) throw new Error(i18n.t("Could not connect this sign-in to the app. Return to TextHalo and try again."));
        const callback = new URL(result.callbackUrl);
        const expected = new URL(redirectUri);
        if (callback.origin !== expected.origin || callback.pathname !== expected.pathname || callback.searchParams.get("state") !== state || !callback.searchParams.get("code")) throw new Error(i18n.t("The sign-in service returned an invalid app callback."));
        window.location.assign(callback.toString());
      }, retry);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("Could not finish sign-in. Please try again."));
      setBusy(false);
    }
  }, [attempt, validRequest, codeChallenge, state, redirectUri, accountEmail]);

  useEffect(() => {
    if (validRequest && !session.isPending && accountEmail) void continueToApp();
  }, [validRequest, session.isPending, accountEmail, continueToApp]);

  return <div className="site-shell"><SiteHeader /><main className="section-wrap signin-page desktop-connect-page"><div className="signin-decoration" aria-hidden="true"><span className="ring-a" /><span className="ring-b" /><div className="privacy-center"><Mark /><span>TEXT HALO</span></div></div><section className="signin-panel"><div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("SECURE DESKTOP SIGN-IN")}</div><h1>{i18n.t("Connect")} <em>TextHalo.</em></h1><p className="signin-intro">{i18n.t("Use your TextHalo account in the Mac app. Your sign-in returns to the app through a one-time, short-lived code.")}</p>
    {!browserReady ? <p role="status">{i18n.t("Checking your sign-in link…")}</p>
      : !validRequest ? <p className="billing-error" role="alert">{i18n.t("This sign-in request is invalid. Start sign-in again from the TextHalo app.")}</p>
      : session.isPending ? <p role="status">{i18n.t("Checking your sign-in…")}</p>
      : !session.data?.user ? <><a className="button button-dark" href={i18n.href(`/sign-in/?returnTo=${encodeURIComponent(returnTo)}`)}>{i18n.t("Sign in to continue")} <span className="arrow">→</span></a><p className="signin-terms">{i18n.t("New to TextHalo?")} <a href={i18n.href(`/sign-in/?mode=sign-up&returnTo=${encodeURIComponent(returnTo)}`)}>{i18n.t("Create an account")}</a></p></>
      : <div className="signed-in-card"><span className="signed-in-check" aria-hidden="true">✓</span><div><strong>{i18n.t("Signed in as")}</strong><span>{session.data.user.email}</span></div>
        <p className="desktop-connect-status" role="status">{error ? i18n.t("We couldn’t return to the app automatically.") : i18n.t("Returning to TextHalo…")}</p>
        {error && <><p className="billing-error" role="alert">{i18n.t(error)}</p><button className="button button-dark button-plan" type="button" disabled={busy} onClick={() => void continueToApp(true)}>{i18n.t("Try again")}<span className="arrow">→</span></button></>}
      </div>}
    <p className="signin-terms">{i18n.t("TextHalo never places your account password or access token in this browser redirect.")}</p>
  </section></main><BillingFooter /></div>;
}

function useBillingStatus() {
  const [status, setStatus] = useState<BillingStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);
  useEffect(() => {
    if (!billingApiUrl) {
      setLoading(false);
      setUnavailable(true);
      return;
    }
    const controller = new AbortController();
    fetch(`${billingApiUrl}/v1/billing/plans`, { headers: billingApiHeaders(), signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error("Billing service unavailable");
        return await response.json() as BillingStatus;
      })
      .then((result) => setStatus(result))
      .catch(() => { if (!controller.signal.aborted) setUnavailable(true); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, []);
  return { status, loading, unavailable };
}

function PricingContent() {
  const i18n = useWebsiteI18n();
  const session = authClient.useSession();
  const billingState = useBillingStatus();
  const billing = billingState.status;
  const [currentPlan, setCurrentPlan] = useState<PlanId | null>(null);
  const [busyPlan, setBusyPlan] = useState<PlanId | null>(null);
  const [busyTopup, setBusyTopup] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [topupError, setTopupError] = useState("");

  useEffect(() => {
    if (!session.data?.user || !billingApiUrl) {
      setCurrentPlan(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const token = await getAccessToken();
        if (!token) return;
        const response = await fetch(`${billingApiUrl}/v1/account`, { headers: billingApiHeaders({ Authorization: `Bearer ${token}` }) });
        if (!response.ok) return;
        const account = await response.json() as BillingAccount;
        const subscription = account.subscription;
        const isSubscribed = Boolean(subscription && ["active", "trialing"].includes(subscription.status) && new Date(subscription.currentPeriodEnd).getTime() > Date.now());
        if (!cancelled) setCurrentPlan(isSubscribed && (subscription?.planId === "plus" || subscription?.planId === "creator") ? subscription.planId : null);
      } catch {
        // Pricing remains explorable when subscription details cannot be loaded.
      }
    })();
    return () => { cancelled = true; };
  }, [session.data?.user?.id]);

  async function choosePlan(planId: PlanId) {
    setError("");
    if (!session.data?.user) {
      window.location.assign(i18n.href(`/sign-in/?plan=${planId}`));
      return;
    }
    setBusyPlan(planId);
    try {
      await startCheckout(planId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t start checkout. Please try again."));
      setBusyPlan(null);
    }
  }

  async function chooseTopup(pack: CreditTopupPack) {
    setTopupError("");
    if (!session.data?.user) {
      window.location.assign(i18n.href(`/sign-in/?${new URLSearchParams({ topup: pack.id }).toString()}`));
      return;
    }
    setBusyTopup(pack.id);
    try {
      await startCreditTopupCheckout(pack.id);
    } catch (cause) {
      setTopupError(cause instanceof Error ? cause.message : i18n.t("We couldn’t start the credit purchase. Please try again."));
      setBusyTopup(null);
    }
  }

  const checkoutUnavailable = billingState.loading || billingState.unavailable || !billingApiUrl || billing?.billingEnabled === false;
  const topupPacks = billing?.topups?.length ? billing.topups : PUBLIC_TOPUP_PACKS;
  const signedIn = Boolean(session.data?.user);
  const topupCheckoutUnavailable = billingState.loading || billingState.unavailable || billing?.billingEnabled !== true || !billing?.topups?.length || !billingApiUrl;
  const planCards = billingState.unavailable || !billing ? FALLBACK_PLANS : billing.plans;

  return <div className="site-shell"><SiteHeader /><main className="pricing-page section-wrap">
    <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("LOCAL FEATURES. OPTIONAL CLOUD CREDITS.")}</div>
    <h1>{i18n.t("Start free.")}<br /><em>{i18n.t("Do more with your words.")}</em></h1>
    <p className="pricing-intro">{i18n.t("Local voices and on-device dictation are free. On-device Assistant is included where Apple Intelligence is available. Add a plan or credit pack for Fish Audio hosted speech and TextHalo Cloud Assistant.")}</p>
    <div className="plan-grid">
      {planCards.map((plan) => {
        const paid = plan.id !== "free";
        const isCurrentPlan = paid && plan.id === currentPlan;
        const missingPlan = paid && billing?.billingEnabled && !billing.plans.some((item) => item.id === plan.id);
        const disabled = paid && (checkoutUnavailable || missingPlan || !plan.price || busyPlan !== null || busyTopup !== null);
        const actionLabel = isCurrentPlan ? i18n.t("Manage current plan") : currentPlan ? i18n.t("Switch to {0}", [plan.name]) : plan.action ? i18n.t(plan.action) : i18n.t("Choose {0}", [plan.name]);
        return <article key={plan.id} className={`plan-card${plan.id === "creator" ? " plan-card-featured" : ""}${isCurrentPlan ? " plan-card-current" : ""}`}>
          {plan.id === "creator" && <span className="plan-ribbon">{i18n.t("FOR YOUR CREATIVE WORK")}</span>}
          {isCurrentPlan && <span className="plan-current-badge">{i18n.t("CURRENT PLAN")}</span>}
          <div className="plan-name">{i18n.t(plan.name)}</div>
          <div className="plan-price">{formatWebsitePlanPrice(plan.price, i18n.language)}</div>
          <p className="plan-description">{plan.id === "free" ? i18n.t("Listen and dictate on your Mac.") : i18n.t(plan.description)}</p>
          <ul>{(plan.id === "free" ? FREE_FEATURES : [...plan.features, i18n.t("Cloud Assistant using the same credit balance")]).map((feature) => <li key={feature}><span aria-hidden="true">✓</span>{formatPlanFeature(feature, plan, i18n.language)}</li>)}</ul>
          {plan.id === "free"
            ? <a className="button button-plan button-plan-light" href={i18n.href(DOWNLOAD)}>{i18n.t("Download TextHalo")} <span className="arrow">→</span></a>
            : <button className={`button button-plan${plan.id === "creator" ? " button-plan-dark" : " button-plan-light"}`} type="button" disabled={disabled} onClick={() => plan.id !== "free" && void choosePlan(plan.id)}>
                {busyPlan === plan.id ? i18n.t("Opening checkout…") : actionLabel}<span className="arrow">→</span>
              </button>}
          {plan.id === "free" && <small className="plan-note">{i18n.t("Requires Apple Silicon")}</small>}
          {paid && <small className="plan-note">{checkoutUnavailable ? i18n.t("Checkout is temporarily unavailable.") : missingPlan ? i18n.t("Checkout setup is pending for this plan.") : !plan.price ? i18n.t("Pricing is temporarily unavailable.") : isCurrentPlan ? i18n.t("Manage your subscription securely with Stripe.") : currentPlan ? i18n.t("Plan changes are confirmed securely with Stripe.") : i18n.t("Checkout is processed securely by Stripe.")}</small>}
        </article>;
      })}
    </div>
    {error && <p className="billing-error" role="alert">{i18n.t(error)}</p>}
    <section className="credit-topup-panel pricing-topup-panel" aria-labelledby="pricing-topup-heading">
      <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("ONE-TIME CLOUD CREDITS")}</div>
      <h2 id="pricing-topup-heading">{i18n.t("More credits, when you need them.")}</h2>
      <p>{i18n.t("Buy credits for hosted speech and Cloud Assistant without starting a monthly plan. Your first top-up includes seven days with two active voice-clone slots; the trial waits while a plan already covers cloning.")}</p>
      <div className="credit-topup-grid">{topupPacks.map((pack) => {
        const price = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(pack.priceCents / 100);
        const estimatedMinutes = Math.round((pack.credits * 1000) / (450 * 900));
        return <article className="credit-topup-option" key={pack.id}>
          <strong>{i18n.t("{0} credits", [pack.credits.toLocaleString(i18n.language)])}</strong>
          <span>{i18n.t("About {0} minutes of English audio*", [estimatedMinutes.toLocaleString(i18n.language)])}</span>
          <button className="button button-dark" type="button" disabled={busyTopup !== null || busyPlan !== null || (signedIn && topupCheckoutUnavailable)} onClick={() => void chooseTopup(pack)}>
            {busyTopup === pack.id ? i18n.t("Opening checkout…") : signedIn ? i18n.t("Buy for {0}", [price]) : i18n.t("Sign in to buy · {0}", [price])}<span className="arrow">→</span>
          </button>
        </article>;
      })}</div>
      {!signedIn && <small className="credit-topup-note">{i18n.t("Sign in or create an account to continue to secure checkout. Packs are available to Free, Plus, and Creator accounts.")}</small>}
      {signedIn && topupCheckoutUnavailable && <p className="credit-topup-unavailable" role="status">{billingState.loading ? i18n.t("Checking credit-pack checkout…") : i18n.t("The checkout service is temporarily unavailable. Your pack selection is shown; please try again shortly.")}</p>}
      <small className="credit-topup-note">{i18n.t("Purchased top-up credits don’t expire. Monthly plan credits are used first and expire at the end of the billing period.")}</small>
      {topupError && <p className="billing-error" role="alert">{i18n.t(topupError)}</p>}
    </section>
    <p className="credit-estimate">{i18n.t("* Audio length is an estimate based on about 900 characters of English per spoken minute. Hosted speech credits are charged by text length, so actual time varies with language, wording, and voice. Cloud Assistant uses credits based on model usage; these audio estimates do not apply to Assistant requests. Reading an answer with a hosted voice uses additional credits.")}</p>
    <section className="local-voice-note"><span className="local-note-mark"><Mark /></span><div><strong>{i18n.t("Local cloning and hosted cloning are different.")}</strong><p>{i18n.t("Chatterbox’s built-in voice cloning runs on your Mac and is included with Free. Saved Fish Audio clones are hosted by Fish Audio. Plan and trial limits are shown in your account and at checkout.")}</p></div></section>
    <p className="pricing-footnote">{i18n.t("Local speech and transcription run on your Mac. On-device Assistant requires macOS 26 or later, a supported Mac, and Apple Intelligence enabled and ready. Cloud Assistant requires sign-in and consent, and uses your available credits. Unused monthly plan credits expire at period end; purchased top-up credits don’t expire.")}</p>
  </main><BillingFooter /></div>;
}

export function PricingPage() {
  return <PricingContent />;
}

function SignInContent() {
  const i18n = useWebsiteI18n();
  const session = authClient.useSession();
  const [mode, setMode] = useState<"sign-in" | "sign-up" | "forgot-password">("sign-in");
  const [plan, setPlan] = useState<PlanId | null>(null);
  const [topupId, setTopupId] = useState<string | null>(null);
  const [returnTo, setReturnTo] = useState<string | null>(null);
  const [sessionExpired, setSessionExpired] = useState(false);
  const [accountNotLinked, setAccountNotLinked] = useState(false);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [turnstileToken, setTurnstileToken] = useState("");
  const [captchaEpoch, setCaptchaEpoch] = useState(0);
  const [googleCaptchaRequested, setGoogleCaptchaRequested] = useState(false);
  const [resetEmailSent, setResetEmailSent] = useState(false);

  useEffect(() => {
    const requestedPlan = new URLSearchParams(window.location.search).get("plan");
    if (requestedPlan === "plus" || requestedPlan === "creator") setPlan(requestedPlan);
    const params = new URLSearchParams(window.location.search);
    setTopupId(params.get("topup"));
    if (params.get("mode") === "forgot-password") setMode("forgot-password");
    if (params.get("mode") === "sign-up") setMode("sign-up");
    setReturnTo(safeInternalReturnTo(params.get("returnTo")));
    setSessionExpired(params.get("reason") === "session-expired");
    setAccountNotLinked(params.get("error") === "account_not_linked");
  }, []);

  async function verifySignupChallenge(token: string) {
    if (isLocalSignup()) return;
    if (!TURNSTILE_SITE_KEY || !billingApiUrl) throw new Error(i18n.t("Signup verification isn’t configured. Please try again later."));
    const response = await fetch(`${billingApiUrl}/v1/auth/turnstile/verify`, {
      method: "POST",
      headers: billingApiHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify({ token, action: "signup" }),
    });
    if (!response.ok) {
      setTurnstileToken("");
      setCaptchaEpoch((epoch) => epoch + 1);
      throw new Error(i18n.t("Please complete the human verification and try again."));
    }
    setTurnstileToken("");
    setCaptchaEpoch((epoch) => epoch + 1);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      if (mode === "sign-up") {
        if (!isLocalSignup() && !turnstileToken) throw new Error(i18n.t("Please complete the human verification before creating your account."));
        if (turnstileToken) await verifySignupChallenge(turnstileToken);
      }
      const result = mode === "sign-in"
        ? await authClient.signIn.email({ email, password })
        : await authClient.signUp.email({ email, password, name: name.trim() || email.split("@")[0] });
      if (result.error) throw new Error(result.error.message || i18n.t("Please check your details and try again."));
      if (returnTo && !accountNotLinked) {
        window.location.assign(i18n.href(returnTo));
        return;
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t sign you in. Please try again."));
    } finally {
      setBusy(false);
    }
  }

  async function requestPasswordReset(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await authClient.requestPasswordReset({
        email,
        redirectTo: `${window.location.origin}${i18n.href("/reset-password/")}`,
      });
      if (result.error) throw new Error(i18n.t("We couldn’t send the reset email. Please try again."));
      setResetEmailSent(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t send the reset email. Please try again."));
    } finally {
      setBusy(false);
    }
  }

  async function completeGoogleSignIn(token?: string) {
    setBusy(true);
    setError("");
    try {
      if (!isLocalSignup()) {
        if (!token) throw new Error(i18n.t("Please complete the human verification to continue with Google."));
        await verifySignupChallenge(token);
      }
      const result = await authClient.signIn.social({
        provider: "google",
        callbackURL: (() => {
          const params = new URLSearchParams();
          if (plan) params.set("plan", plan);
          if (topupId) params.set("topup", topupId);
          if (returnTo) params.set("returnTo", returnTo);
          const query = params.toString();
          return `${window.location.origin}${i18n.href("/sign-in/")}${query ? `?${query}` : ""}`;
        })(),
      });
      if (result.error) throw new Error(result.error.message || i18n.t("Google sign-in could not be started."));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("Google sign-in could not be started."));
      setBusy(false);
      setGoogleCaptchaRequested(false);
    }
  }

  function signInWithGoogle() {
    if (isLocalSignup()) {
      void completeGoogleSignIn();
      return;
    }
    if (turnstileToken) void completeGoogleSignIn(turnstileToken);
    else setGoogleCaptchaRequested(true);
  }

  async function continueToCheckout() {
    if (!plan) return;
    setBusy(true);
    setError("");
    try {
      await startCheckout(plan);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t start checkout. Please try again."));
      setBusy(false);
    }
  }

  async function continueToTopup() {
    if (!topupId) return;
    setBusy(true);
    setError("");
    try {
      await startCreditTopupCheckout(topupId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t start the credit purchase. Please try again."));
      setBusy(false);
    }
  }

  async function signOut() {
    await authClient.signOut();
    window.location.assign(i18n.href("/pricing/"));
  }

  const selectedName = plan === "plus" ? "Plus" : plan === "creator" ? "Creator" : null;
  const signedInUser = session.data?.user;
  const onTurnstileToken = (token: string) => {
    setTurnstileToken(token);
    if (token && googleCaptchaRequested) void completeGoogleSignIn(token);
  };

  return <div className="site-shell"><SiteHeader /><main className="signin-page section-wrap">
    <div className="signin-decoration"><div className="privacy-ring ring-a"/><div className="privacy-ring ring-b"/><div className="privacy-center"><Mark /><span>TEXT HALO</span></div></div>
    <section className="signin-panel">
      <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("YOUR WORDS, YOUR ACCOUNT")}</div>
      <h1>{mode === "forgot-password" ? <>{i18n.t("Forgot your")}<br /><em>{i18n.t("password?")}</em></> : selectedName ? <>{i18n.t("One small step")}<br /><em>{i18n.t("to {0}.", [selectedName])}</em></> : <>{i18n.t("Welcome")}<br /><em>{i18n.t("to TextHalo.")}</em></>}</h1>
      <p className="signin-intro">{mode === "forgot-password" ? i18n.t("Enter your account email and we’ll send a link to reset your password.") : selectedName ? i18n.t("Sign in or create your account to continue to the {0} checkout.", [selectedName]) : topupId ? i18n.t("Sign in or create your account to continue to your one-time hosted audio credit pack.") : i18n.t("Sign in to manage your hosted voices and monthly credits.")}</p>
      {signedInUser ? <div className="signed-in-card">
        <span className="signed-in-check">✓</span><div><strong>{i18n.t("You’re signed in")}</strong><span>{signedInUser.email}</span></div>
        {accountNotLinked && <p className="signin-account-alert" role="status">{i18n.t("Google sign-in couldn’t be linked because Neon Auth rejected the callback. Your TextHalo account is still signed in and works with your password.")}</p>}
        {error && <p className="billing-error" role="alert">{i18n.t(error)}</p>}
        {plan ? <button className="button button-dark button-plan" disabled={busy} onClick={() => void continueToCheckout()}>{busy ? i18n.t("Opening checkout…") : i18n.t("Continue to {0} checkout", [selectedName])}<span className="arrow">→</span></button> : topupId ? <button className="button button-dark button-plan" disabled={busy} onClick={() => void continueToTopup()}>{busy ? i18n.t("Opening checkout…") : i18n.t("Continue to credit pack checkout")}<span className="arrow">→</span></button> : returnTo ? <a className="button button-dark button-plan" href={i18n.href(returnTo)}>{i18n.t("Continue to your account")} <span className="arrow">→</span></a> : <a className="button button-dark button-plan" href={i18n.href("/pricing/")}>{i18n.t("View plans")} <span className="arrow">→</span></a>}
        <button className="signin-signout" type="button" onClick={() => void signOut()}>{i18n.t("Sign out")}</button>
      </div> : <>
        {mode === "forgot-password" ? resetEmailSent ? <div className="signed-in-card reset-confirmation"><strong>{i18n.t("Check your email")}</strong><span>{i18n.t("If there’s an account for {0}, a password reset link is on its way.", [email])}</span></div> : <form className="signin-form" onSubmit={(event) => void requestPasswordReset(event)}>
          <label>{i18n.t("Email address")}<input type="email" autoComplete="email" required value={email} onChange={(event) => setEmail(event.target.value)} /></label>
          <button className="button button-dark button-plan" type="submit" disabled={busy}>{busy ? i18n.t("Sending link…") : i18n.t("Send reset link")}<span className="arrow">→</span></button>
        </form> : <>
          <div className="signin-tabs" role="tablist" aria-label={i18n.t("Account access")}>
            <button type="button" role="tab" aria-selected={mode === "sign-in"} onClick={() => { setMode("sign-in"); setError(""); }}>{i18n.t("Sign in")}</button>
            <button type="button" role="tab" aria-selected={mode === "sign-up"} onClick={() => { setMode("sign-up"); setError(""); }}>{i18n.t("Create account")}</button>
          </div>
          <form className="signin-form" onSubmit={(event) => void submit(event)}>
            {mode === "sign-up" && <label>{i18n.t("Your name")}<input autoComplete="name" value={name} onChange={(event) => setName(event.target.value)} maxLength={100} /></label>}
            <label>{i18n.t("Email address")}<input type="email" autoComplete="email" required value={email} onChange={(event) => setEmail(event.target.value)} /></label>
            {mode === "sign-in" && <label>{i18n.t("Password")}<input type="password" autoComplete="current-password" required minLength={8} value={password} onChange={(event) => setPassword(event.target.value)} /></label>}
            {mode === "sign-up" && <label>{i18n.t("Password")}<input type="password" autoComplete="new-password" required minLength={8} value={password} onChange={(event) => setPassword(event.target.value)} /></label>}
            <TurnstileWidget key={captchaEpoch} active={mode === "sign-up" || googleCaptchaRequested} onToken={onTurnstileToken} />
            <button className="button button-dark button-plan" type="submit" disabled={busy}>{busy ? i18n.t("Please wait…") : mode === "sign-in" ? i18n.t("Sign in with email") : i18n.t("Create your account")}<span className="arrow">→</span></button>
          </form>
          {mode === "sign-in" && <button type="button" className="signin-forgot" onClick={() => { setMode("forgot-password"); setError(""); setResetEmailSent(false); }}>{i18n.t("Forgot password?")}</button>}
          {!accountNotLinked && <><div className="signin-divider"><span /> {i18n.t("or")} <span /></div>
          <button type="button" className="button google-button" disabled={busy} onClick={signInWithGoogle}><span className="google-g">G</span> {i18n.t("Continue with Google")}</button></>}
        </>}
        {sessionExpired && !signedInUser && <p className="signin-config-note" role="status">{i18n.t("Your session expired. Sign in again to continue.")}</p>}
        {accountNotLinked && !signedInUser && <p className="signin-config-note" role="status">{i18n.t("Google sign-in isn’t available for this existing password account right now: Neon Auth is rejecting the OAuth callback. Sign in with your email and password below to continue.")}</p>}
        {error && <p className="billing-error" role="alert">{i18n.t(error)}</p>}
        {mode === "forgot-password" ? <button type="button" className="text-link signin-back" onClick={() => { setMode("sign-in"); setError(""); }}>{i18n.t("← Back to sign in")}</button> : <p className="signin-terms">{i18n.t("TextHalo uses Neon Auth to manage your account and sign-in session.")}</p>}
      </>}
      {!billingApiUrl && selectedName && <p className="signin-config-note">{i18n.t("Checkout connection is not configured on this website.")}</p>}
      <a className="text-link signin-back" href={i18n.href("/pricing/")}>{i18n.t("← Back to plans")}</a>
    </section>
  </main><BillingFooter /></div>;
}

export function SignInPage() {
  return <SignInContent />;
}

export function PasswordResetPage() {
  const i18n = useWebsiteI18n();
  const [status, setStatus] = useState<"checking" | "ready" | "invalid" | "updated">("checking");
  const [token, setToken] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const resetToken = params.get("token");
    if (!resetToken || params.has("error")) setStatus("invalid");
    else {
      setToken(resetToken);
      setStatus("ready");
    }
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password !== confirmPassword) {
      setError(i18n.t("Those passwords don’t match."));
      return;
    }
    setBusy(true);
    setError("");
    try {
      const result = await authClient.resetPassword({ newPassword: password, token });
      if (result.error) throw new Error(i18n.t("This reset link is invalid or expired. Request a new one to continue."));
      setStatus("updated");
      window.history.replaceState(null, "", "/reset-password/");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t reset your password. Please request a new link."));
    } finally {
      setBusy(false);
    }
  }

  return <div className="site-shell"><SiteHeader /><main className="signin-page section-wrap">
    <div className="signin-decoration"><div className="privacy-ring ring-a"/><div className="privacy-ring ring-b"/><div className="privacy-center"><Mark /><span>TEXT HALO</span></div></div>
    <section className="signin-panel">
      <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("ACCOUNT SECURITY")}</div>
      {status === "updated" ? <><h1>{i18n.t("Password")}<br /><em>{i18n.t("updated.")}</em></h1><p className="signin-intro">{i18n.t("Your password has been changed. Sign in with your new password.")}</p><a className="button button-dark button-plan" href={i18n.href("/sign-in/")}>{i18n.t("Back to sign in")} <span className="arrow">→</span></a></> : status === "invalid" ? <><h1>{i18n.t("That link")}<br /><em>{i18n.t("has expired.")}</em></h1><p className="signin-intro">{i18n.t("Request a new password reset link and we’ll send you a fresh one.")}</p><a className="button button-dark button-plan" href={i18n.href("/sign-in/?mode=forgot-password")}>{i18n.t("Request another link")} <span className="arrow">→</span></a></> : <>
        <h1>{i18n.t("Choose a new")}<br /><em>{i18n.t("password.")}</em></h1>
        <p className="signin-intro">{i18n.t("Use at least 8 characters for your new password.")}</p>
        {status === "ready" && <form className="signin-form" onSubmit={(event) => void submit(event)}>
          <label>{i18n.t("New password")}<input type="password" autoComplete="new-password" required minLength={8} maxLength={128} value={password} onChange={(event) => setPassword(event.target.value)} /></label>
          <label>{i18n.t("Confirm new password")}<input type="password" autoComplete="new-password" required minLength={8} maxLength={128} value={confirmPassword} onChange={(event) => setConfirmPassword(event.target.value)} /></label>
          <button className="button button-dark button-plan" type="submit" disabled={busy}>{busy ? i18n.t("Updating password…") : i18n.t("Update password")}<span className="arrow">→</span></button>
        </form>}
        {error && <p className="billing-error" role="alert">{i18n.t(error)}</p>}
      </>}
      <a className="text-link signin-back" href={i18n.href("/sign-in/")}>{i18n.t("← Back to sign in")}</a>
    </section>
  </main><BillingFooter /></div>;
}

function BillingSuccessContent() {
  const i18n = useWebsiteI18n();
  const session = authClient.useSession();
  const billing = useBillingStatus().status;
  const [credits, setCredits] = useState<number | null>(null);
  const [creditBreakdown, setCreditBreakdown] = useState<CreditBalanceBreakdown | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [busyTopup, setBusyTopup] = useState<string | null>(null);
  const [topupError, setTopupError] = useState("");

  async function refreshBalance() {
    if (!billingApiUrl || !session.data?.user) return;
    setLoading(true);
    setError("");
    try {
      const token = await getAccessToken();
      const returnTo = `${window.location.pathname}${window.location.search}`;
      if (!token) {
        await promptSignIn({ returnTo });
        return;
      }
      const response = await fetch(`${billingApiUrl}/v1/account`, { headers: billingApiHeaders({ Authorization: `Bearer ${token}` }) });
      if (response.status === 401) {
        await promptSignIn({ returnTo });
        return;
      }
      if (!response.ok) throw new Error(i18n.t("We couldn’t read your account balance yet."));
      const account = await response.json() as { availableCredits?: number; creditBreakdown?: CreditBalanceBreakdown };
      setCredits(typeof account.availableCredits === "number" ? account.availableCredits : null);
      setCreditBreakdown(account.creditBreakdown ?? null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t read your account balance yet."));
    } finally {
      setLoading(false);
    }
  }

  async function buyCredits(pack: CreditTopupPack) {
    setBusyTopup(pack.id);
    setTopupError("");
    try {
      await startCreditTopupCheckout(pack.id);
    } catch (cause) {
      setTopupError(cause instanceof Error ? cause.message : i18n.t("We couldn’t start the credit purchase. Please try again."));
      setBusyTopup(null);
    }
  }

  useEffect(() => {
    if (session.data?.user) void refreshBalance();
  }, [session.data?.user?.id]);

  return <div className="site-shell"><SiteHeader /><main className="billing-success section-wrap">
    <div className="success-orbit"><span>✓</span></div>
    <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("THANK YOU FOR CHOOSING TEXTHALO")}</div>
    <h1>{i18n.t("You’re back.")}<br /><em>{i18n.t("Let’s listen.")}</em></h1>
    <p className="success-copy">{i18n.t("Stripe has returned you to TextHalo. Your hosted speech credits are added after Stripe confirms the payment.")}</p>
    {session.data?.user ? <>
      <div className="balance-card"><span>{i18n.t("AVAILABLE CREDITS")}</span><strong>{credits === null ? (loading ? i18n.t("Checking…") : "—") : credits.toLocaleString(i18n.language)}</strong><small className="balance-owner">{i18n.t("Balance for")} <strong>{session.data.user.email}</strong></small>
        {creditBreakdown && <div className="credit-breakdown" aria-label={i18n.t("Credit balance details")}>
          <span>{i18n.t("Monthly plan remaining")}<strong>{creditBreakdown.planCredits.toLocaleString(i18n.language)}</strong></span>
          <span>{i18n.t("Top-up credits remaining")}<strong>{creditBreakdown.topupCredits.toLocaleString(i18n.language)}</strong></span>
          {creditBreakdown.otherCredits > 0 && <span>{i18n.t("Other credits")}<strong>{creditBreakdown.otherCredits.toLocaleString(i18n.language)}</strong></span>}
          {(creditBreakdown.debtCredits ?? 0) > 0 && <span>{i18n.t("Refund balance due")}<strong>{creditBreakdown.debtCredits!.toLocaleString(i18n.language)}</strong></span>}
        </div>}
        {credits === 0 && <small>{i18n.t("Credits may take a moment to appear while Stripe sends its payment confirmation.")}</small>}<button className="text-link" type="button" disabled={loading} onClick={() => void refreshBalance()}>{loading ? i18n.t("Refreshing…") : i18n.t("Refresh balance")}<span className="arrow">→</span></button>
      </div>
      {billing?.billingEnabled && billing.topups && billing.topups.length > 0 && <section className="credit-topup-panel" aria-labelledby="topup-heading">
        <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("NEED MORE HOSTED AUDIO?")}</div>
        <h2 id="topup-heading">{i18n.t("Add a credit pack.")}</h2>
        <p>{i18n.t("One-time purchases work with Free, Plus, and Creator. The first top-up includes seven days with two active voice-clone slots, paused while a plan covers cloning.")}</p>
        <div className="credit-topup-grid">{billing.topups.map((pack) => {
          const price = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(pack.priceCents / 100);
          const estimatedMinutes = Math.round((pack.credits * 1000) / (450 * 900));
          return <article className="credit-topup-option" key={pack.id}>
            <strong>{i18n.t("{0} credits", [pack.credits.toLocaleString(i18n.language)])}</strong>
            <span>{i18n.t("About {0} minutes of English audio*", [estimatedMinutes.toLocaleString(i18n.language)])}</span>
            <button className="button button-dark" type="button" disabled={busyTopup !== null} onClick={() => void buyCredits(pack)}>
              {busyTopup === pack.id ? i18n.t("Opening checkout…") : i18n.t("Buy for {0}", [price])}<span className="arrow">→</span>
            </button>
          </article>;
        })}</div>
        <small className="credit-topup-note">{i18n.t("*Audio time is an estimate. Actual length varies with text, language, and voice.")}</small>
        {topupError && <p className="billing-error" role="alert">{i18n.t(topupError)}</p>}
      </section>}
    </> : <a className="button button-dark button-large success-signin" href={i18n.href("/sign-in/")}>{i18n.t("Sign in to view your account")} <span className="arrow">→</span></a>}
    {error && <p className="billing-error" role="alert">{i18n.t(error)}</p>}
    <div className="success-actions"><a className="button button-dark button-large" href={i18n.href("/account/billing/")}>{i18n.t("View billing account")} <span className="arrow">→</span></a><a className="text-link" href={i18n.href("/")}>{i18n.t("Return home")} <span className="arrow">→</span></a></div>
  </main><BillingFooter /></div>;
}

export function BillingSuccessPage() {
  return <BillingSuccessContent />;
}

function BillingAccountContent() {
  const i18n = useWebsiteI18n();
  const session = authClient.useSession();
  const billingState = useBillingStatus();
  const [account, setAccount] = useState<BillingAccount | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [busy, setBusy] = useState(false);
  const [busyTopup, setBusyTopup] = useState<string | null>(null);

  async function refreshAccount() {
    if (!billingApiUrl || !session.data?.user) return;
    setLoading(true);
    setError("");
    try {
      const token = await getAccessToken();
      if (!token) {
        await promptSignIn({ returnTo: "/account/billing/" });
        return;
      }
      const response = await fetch(`${billingApiUrl}/v1/account`, { headers: billingApiHeaders({ Authorization: `Bearer ${token}` }) });
      if (response.status === 401) {
        await promptSignIn({ returnTo: "/account/billing/" });
        return;
      }
      if (!response.ok) throw new Error(i18n.t("We couldn’t load your account details. Please try again."));
      setAccount(await response.json() as BillingAccount);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : i18n.t("We couldn’t load your account details. Please try again."));
    } finally {
      setLoading(false);
    }
  }

  async function manageSubscription(planId: string) {
    if (planId !== "plus" && planId !== "creator") return;
    setBusy(true);
    setActionError("");
    try {
      await startCheckout(planId);
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : i18n.t("We couldn’t open your billing settings. Please try again."));
      setBusy(false);
    }
  }

  async function buyCredits(pack: CreditTopupPack) {
    setBusyTopup(pack.id);
    setActionError("");
    try {
      await startCreditTopupCheckout(pack.id);
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : i18n.t("We couldn’t start the credit purchase. Please try again."));
      setBusyTopup(null);
    }
  }

  useEffect(() => {
    if (session.data?.user) void refreshAccount();
  }, [session.data?.user?.id]);

  const subscription = account?.subscription ?? null;
  const planName = subscription?.planId === "plus" ? "Plus" : subscription?.planId === "creator" ? "Creator" : null;
  const periodEnd = subscription?.currentPeriodEnd ? new Date(subscription.currentPeriodEnd) : null;
  const hasActivePlan = Boolean(subscription && ["active", "trialing"].includes(subscription.status) && new Date(subscription.currentPeriodEnd).getTime() > Date.now());

  return <div className="site-shell"><SiteHeader /><main className="billing-success billing-account-page section-wrap">
    <div className="success-orbit"><span>◉</span></div>
    <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("YOUR TEXTHALO ACCOUNT")}</div>
    <h1>{i18n.t("Credits &")}<br /><em>{i18n.t("billing.")}</em></h1>
    <p className="success-copy">{i18n.t("Plan details and hosted audio credits for")} {session.data?.user?.email ?? i18n.t("your account")}.</p>
    {session.isPending ? <p className="billing-account-message">{i18n.t("Checking your sign-in…")}</p> : session.data?.user ? <>
      <section className="balance-card billing-account-balance" aria-label={i18n.t("Credit balance")}>
        <span>{i18n.t("AVAILABLE CREDITS")}</span>
        <strong>{account?.availableCredits === undefined ? loading ? i18n.t("Checking…") : "—" : account.availableCredits.toLocaleString(i18n.language)}</strong>
        <small className="balance-owner">{i18n.t("Balance for")} <strong>{session.data.user.email}</strong></small>
        {account?.creditBreakdown && <div className="credit-breakdown" aria-label={i18n.t("Credit balance details")}>
          <span>{i18n.t("Monthly plan remaining")}<strong>{account.creditBreakdown.planCredits.toLocaleString(i18n.language)}</strong></span>
          <span>{i18n.t("Top-up credits remaining")}<strong>{account.creditBreakdown.topupCredits.toLocaleString(i18n.language)}</strong></span>
          {account.creditBreakdown.otherCredits > 0 && <span>{i18n.t("Other credits")}<strong>{account.creditBreakdown.otherCredits.toLocaleString(i18n.language)}</strong></span>}
          {(account.creditBreakdown.debtCredits ?? 0) > 0 && <span>{i18n.t("Refund balance due")}<strong>{account.creditBreakdown.debtCredits!.toLocaleString(i18n.language)}</strong></span>}
        </div>}
        <button className="text-link" type="button" disabled={loading} onClick={() => void refreshAccount()}>{loading ? i18n.t("Refreshing…") : i18n.t("Refresh balance")}<span className="arrow">→</span></button>
      </section>
      <section className="account-plan-card" aria-labelledby="account-plan-heading">
        <div className="eyebrow"><span className="eyebrow-line" /> SUBSCRIPTION</div>
        <h2 id="account-plan-heading">{loading && !account ? i18n.t("Loading your plan…") : account ? planName ?? i18n.t("Free plan") : i18n.t("Plan details unavailable")}</h2>
        {hasActivePlan && subscription ? <>
          <p>{i18n.t("Status:")} <strong>{i18n.t(subscription.status.replace(/_/g, " "))}</strong>{periodEnd && !Number.isNaN(periodEnd.getTime()) ? <> · {subscription.cancelAtPeriodEnd ? i18n.t("Access through ") : i18n.t("Renews ")}{periodEnd.toLocaleDateString(i18n.language, { year: "numeric", month: "long", day: "numeric" })}</> : null}</p>
          <div className="account-plan-actions">
            <button className="button button-dark" type="button" disabled={busy || busyTopup !== null} onClick={() => void manageSubscription(subscription.planId)}>{busy ? i18n.t("Opening Stripe…") : i18n.t("Manage subscription")}<span className="arrow">→</span></button>
            {subscription.planId === "plus" && <button className="button button-plan-light" type="button" disabled={busy || busyTopup !== null} onClick={() => void manageSubscription("creator")}>{i18n.t("Upgrade to Creator")} <span className="arrow">→</span></button>}
          </div>
        </> : account && subscription && planName ? <><p>{i18n.t("Your {0} subscription is", [planName])} <strong>{i18n.t(subscription.status.replace(/_/g, " "))}</strong>{i18n.t(". Hosted plan access is paused until the billing issue is resolved.")}</p><button className="button button-dark" type="button" disabled={busy} onClick={() => void manageSubscription(subscription.planId)}>{busy ? i18n.t("Opening Stripe…") : i18n.t("Manage subscription")}<span className="arrow">→</span></button></> : account ? <><p>{i18n.t("You’re using the Free plan. Choose a hosted audio plan whenever you need Fish Audio voices and monthly credits.")}</p><a className="button button-plan-light" href={i18n.href("/pricing/")}>{i18n.t("Compare plans")} <span className="arrow">→</span></a></> : null}
      </section>
      {billingState.status?.billingEnabled && billingState.status.topups && billingState.status.topups.length > 0 ? <section className="credit-topup-panel" aria-labelledby="account-topup-heading">
        <div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("ONE-TIME CLOUD CREDITS")}</div>
        <h2 id="account-topup-heading">{i18n.t("Add a credit pack.")}</h2>
        <p>{i18n.t("Top-up credits are separate from your monthly plan balance and don’t expire. Your first purchase includes seven days with two active voice-clone slots, paused while a plan covers cloning.")}</p>
        <div className="credit-topup-grid">{billingState.status.topups.map((pack) => {
          const price = new Intl.NumberFormat(i18n.language, { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(pack.priceCents / 100);
          const estimatedMinutes = Math.round((pack.credits * 1000) / (450 * 900));
          return <article className="credit-topup-option" key={pack.id}><strong>{i18n.t("{0} credits", [pack.credits.toLocaleString(i18n.language)])}</strong><span>{i18n.t("About {0} minutes of English audio*", [estimatedMinutes.toLocaleString(i18n.language)])}</span><button className="button button-dark" type="button" disabled={busyTopup !== null || busy} onClick={() => void buyCredits(pack)}>{busyTopup === pack.id ? i18n.t("Opening checkout…") : i18n.t("Buy for {0}", [price])}<span className="arrow">→</span></button></article>;
        })}</div>
        <small className="credit-topup-note">{i18n.t("Monthly plan credits are used first and expire at the end of your billing period. Top-up credits don’t expire.")}</small>
      </section> : <section className="credit-topup-panel" aria-labelledby="account-topup-heading"><div className="eyebrow"><span className="eyebrow-line" /> {i18n.t("ONE-TIME CLOUD CREDITS")}</div><h2 id="account-topup-heading">{i18n.t("Add a credit pack.")}</h2><p className="credit-topup-unavailable" role="status">{billingState.loading ? i18n.t("Loading available credit packs…") : billingState.unavailable ? i18n.t("Credit packs are temporarily unavailable while the billing service reconnects. Please refresh in a moment.") : i18n.t("One-time credit packs are not available right now.")}</p></section>}
    </> : <><p className="billing-account-message">{i18n.t("Sign in to see the plan and credits for your account.")}</p><a className="button button-dark button-large success-signin" href={i18n.href("/sign-in/?returnTo=%2Faccount%2Fbilling%2F")}>{i18n.t("Sign in to view billing")} <span className="arrow">→</span></a></>}
    {error && <p className="billing-error" role="alert">{i18n.t(error)}</p>}
    {actionError && <p className="billing-error" role="alert">{i18n.t(actionError)}</p>}
    <div className="success-actions"><a className="text-link" href={i18n.href("/pricing/")}>{i18n.t("Change plan")} <span className="arrow">→</span></a><a className="text-link" href={i18n.href("/")}>{i18n.t("Return home")} <span className="arrow">→</span></a></div>
  </main><BillingFooter /></div>;
}

export function BillingAccountPage() {
  return <BillingAccountContent />;
}
