import { useEffect, useState, type FormEvent } from "react";
import { authClient, billingApiUrl, getAccessToken, startCheckout } from "./auth";

type PlanId = "plus" | "creator";
type Plan = { id: PlanId; creditsPerPeriod: number };
type BillingStatus = { billingEnabled: boolean; plans: Plan[] };

const PLANS = [
  {
    id: "free" as const,
    name: "Free",
    price: "$0",
    description: "A calm place to start listening.",
    features: [
      "Apple native voices",
      "Kokoro local AI voices",
      "Chatterbox with built-in local voice cloning",
      "Speech generated on your Mac",
    ],
    action: "Download TextHalo",
  },
  {
    id: "plus" as const,
    name: "Plus",
    price: "$9.99",
    description: "Hosted voices when you want a faster path.",
    features: [
      "Everything in Free",
      "Fish Audio hosted voices",
      "80,000 credits each month",
      "About 195 minutes of spoken English*",
    ],
    action: "Choose Plus",
  },
  {
    id: "creator" as const,
    name: "Creator",
    price: "$19.99",
    description: "More hosted audio, with saved Fish clones.",
    features: [
      "Everything in Free",
      "Fish Audio hosted voices",
      "150,000 credits each month",
      "About 370 minutes of spoken English*",
      "Up to five saved Fish Audio clones",
    ],
    action: "Choose Creator",
  },
];

const DOWNLOAD = "https://github.com/bharat2808/texthalo/releases/latest/download/TextHalo-macOS-aarch64.dmg";

function Mark() {
  return <span className="brand-mark" aria-hidden="true"><span /></span>;
}

function BillingHeader() {
  return <>
    <div className="announcement"><span className="announcement-dot" /> TextHalo is open source <span className="announcement-separator">·</span> Made for macOS</div>
    <header className="site-header billing-header">
      <a className="wordmark" href="/"><Mark /><span>TextHalo</span></a>
      <nav aria-label="Main navigation">
        <a href="/#how-it-works">How it works</a>
        <a href="/#voices">Voices</a>
        <a href="/pricing/" aria-current="page">Pricing</a>
        <a className="button button-dark" href={DOWNLOAD}>Get TextHalo <span className="arrow">→</span></a>
      </nav>
    </header>
  </>;
}

function BillingFooter() {
  return <footer className="site-footer"><a className="wordmark footer-wordmark" href="/"><Mark /><span>TextHalo</span></a><span className="footer-copy">A little more room to listen.</span><div className="footer-links"><a href="/">Home</a><a href="/demo/">Demo</a><a href="https://github.com/bharat2808/texthalo">GitHub <span className="arrow">↗</span></a></div><span className="copyright">© {new Date().getFullYear()} TextHalo</span></footer>;
}

function useBillingStatus() {
  const [status, setStatus] = useState<BillingStatus | null>(null);
  useEffect(() => {
    if (!billingApiUrl) return;
    const controller = new AbortController();
    fetch(`${billingApiUrl}/v1/billing/plans`, { signal: controller.signal })
      .then((response) => response.ok ? response.json() as Promise<BillingStatus> : null)
      .then((result) => { if (result) setStatus(result); })
      .catch(() => undefined);
    return () => controller.abort();
  }, []);
  return status;
}

function PricingContent() {
  const session = authClient.useSession();
  const billing = useBillingStatus();
  const [busyPlan, setBusyPlan] = useState<PlanId | null>(null);
  const [error, setError] = useState("");

  async function choosePlan(planId: PlanId) {
    setError("");
    if (!session.data?.user) {
      window.location.assign(`/sign-in/?plan=${planId}`);
      return;
    }
    setBusyPlan(planId);
    try {
      await startCheckout(planId);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "We couldn’t start checkout. Please try again.");
      setBusyPlan(null);
    }
  }

  const checkoutUnavailable = billing?.billingEnabled === false;

  return <div className="site-shell"><BillingHeader /><main className="pricing-page section-wrap">
    <div className="eyebrow"><span className="eyebrow-line" /> A VOICE THAT FITS YOUR WORKFLOW</div>
    <h1>Start free.<br /><em>Listen for longer.</em></h1>
    <p className="pricing-intro">Every plan includes TextHalo’s local voices. Add Fish Audio hosted generation when you want the convenience of cloud voices and monthly usage credits.</p>
    <div className="plan-grid">
      {PLANS.map((plan) => {
        const paid = plan.id !== "free";
        const missingPlan = paid && billing?.billingEnabled && !billing.plans.some((item) => item.id === plan.id);
        const disabled = paid && (checkoutUnavailable || missingPlan || busyPlan !== null);
        return <article key={plan.id} className={`plan-card${plan.id === "creator" ? " plan-card-featured" : ""}`}>
          {plan.id === "creator" && <span className="plan-ribbon">FOR YOUR CREATIVE WORK</span>}
          <div className="plan-name">{plan.name}</div>
          <div className="plan-price">{plan.price}<span>{paid ? "/ month" : ""}</span></div>
          <p className="plan-description">{plan.description}</p>
          <ul>{plan.features.map((feature) => <li key={feature}><span aria-hidden="true">✓</span>{feature}</li>)}</ul>
          {plan.id === "free"
            ? <a className="button button-plan button-plan-light" href={DOWNLOAD}>Download TextHalo <span className="arrow">→</span></a>
            : <button className={`button button-plan${plan.id === "creator" ? " button-plan-dark" : " button-plan-light"}`} type="button" disabled={disabled} onClick={() => void choosePlan(plan.id)}>
                {busyPlan === plan.id ? "Opening checkout…" : plan.action}<span className="arrow">→</span>
              </button>}
          {paid && <small className="plan-note">{checkoutUnavailable ? "Checkout is temporarily unavailable." : missingPlan ? "Checkout setup is pending for this plan." : "Monthly checkout is processed securely by Stripe."}</small>}
        </article>;
      })}
    </div>
    {error && <p className="billing-error" role="alert">{error}</p>}
    <p className="credit-estimate">* Audio length is an estimate based on about 900 characters of English per spoken minute. Credits are charged by text length, so actual time varies with language, wording, and voice.</p>
    <section className="local-voice-note"><span className="local-note-mark"><Mark /></span><div><strong>Local cloning and hosted cloning are different.</strong><p>Chatterbox’s built-in voice cloning runs on your Mac and is included with Free. Saved Fish Audio clones are hosted by Fish Audio and are included with Creator, up to five.</p></div></section>
    <p className="pricing-footnote">Local speech synthesis runs on your Mac; its speed depends on your device. Hosted Fish Audio credits refresh each billing period and unused credits expire at period end.</p>
  </main><BillingFooter /></div>;
}

export function PricingPage() {
  return <PricingContent />;
}

function SignInContent() {
  const session = authClient.useSession();
  const [mode, setMode] = useState<"sign-in" | "sign-up">("sign-in");
  const [plan, setPlan] = useState<PlanId | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const requestedPlan = new URLSearchParams(window.location.search).get("plan");
    if (requestedPlan === "plus" || requestedPlan === "creator") setPlan(requestedPlan);
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = mode === "sign-in"
        ? await authClient.signIn.email({ email, password })
        : await authClient.signUp.email({ email, password, name: name.trim() || email.split("@")[0] });
      if (result.error) throw new Error(result.error.message || "Please check your details and try again.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "We couldn’t sign you in. Please try again.");
    } finally {
      setBusy(false);
    }
  }

  async function signInWithGoogle() {
    setBusy(true);
    setError("");
    try {
      const result = await authClient.signIn.social({
        provider: "google",
        callbackURL: plan ? `${window.location.origin}/sign-in/?plan=${plan}` : `${window.location.origin}/sign-in/`,
      });
      if (result.error) throw new Error(result.error.message || "Google sign-in could not be started.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Google sign-in could not be started.");
      setBusy(false);
    }
  }

  async function continueToCheckout() {
    if (!plan) return;
    setBusy(true);
    setError("");
    try {
      await startCheckout(plan);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "We couldn’t start checkout. Please try again.");
      setBusy(false);
    }
  }

  async function signOut() {
    await authClient.signOut();
    window.location.assign("/pricing/");
  }

  const selectedName = plan === "plus" ? "Plus" : plan === "creator" ? "Creator" : null;
  const signedInUser = session.data?.user;

  return <div className="site-shell"><BillingHeader /><main className="signin-page section-wrap">
    <div className="signin-decoration"><div className="privacy-ring ring-a"/><div className="privacy-ring ring-b"/><div className="privacy-center"><Mark /><span>TEXT HALO</span></div></div>
    <section className="signin-panel">
      <div className="eyebrow"><span className="eyebrow-line" /> YOUR WORDS, YOUR ACCOUNT</div>
      <h1>{selectedName ? <>One small step<br /><em>to {selectedName}.</em></> : <>Welcome<br /><em>to TextHalo.</em></>}</h1>
      <p className="signin-intro">{selectedName ? `Sign in or create your account to continue to the ${selectedName} checkout.` : "Sign in to manage your hosted voices and monthly credits."}</p>
      {signedInUser ? <div className="signed-in-card">
        <span className="signed-in-check">✓</span><div><strong>You’re signed in</strong><span>{signedInUser.email}</span></div>
        {plan ? <button className="button button-dark button-plan" disabled={busy} onClick={() => void continueToCheckout()}>{busy ? "Opening checkout…" : `Continue to ${selectedName} checkout`}<span className="arrow">→</span></button> : <a className="button button-dark button-plan" href="/pricing/">View plans <span className="arrow">→</span></a>}
        <button className="signin-signout" type="button" onClick={() => void signOut()}>Sign out</button>
      </div> : <>
        <div className="signin-tabs" role="tablist" aria-label="Account access">
          <button type="button" role="tab" aria-selected={mode === "sign-in"} onClick={() => { setMode("sign-in"); setError(""); }}>Sign in</button>
          <button type="button" role="tab" aria-selected={mode === "sign-up"} onClick={() => { setMode("sign-up"); setError(""); }}>Create account</button>
        </div>
        <form className="signin-form" onSubmit={(event) => void submit(event)}>
          {mode === "sign-up" && <label>Your name<input autoComplete="name" value={name} onChange={(event) => setName(event.target.value)} maxLength={100} /></label>}
          <label>Email address<input type="email" autoComplete="email" required value={email} onChange={(event) => setEmail(event.target.value)} /></label>
          <label>Password<input type="password" autoComplete={mode === "sign-in" ? "current-password" : "new-password"} required minLength={8} value={password} onChange={(event) => setPassword(event.target.value)} /></label>
          <button className="button button-dark button-plan" type="submit" disabled={busy}>{busy ? "Please wait…" : mode === "sign-in" ? "Sign in with email" : "Create your account"}<span className="arrow">→</span></button>
        </form>
        <div className="signin-divider"><span /> or <span /></div>
        <button type="button" className="button google-button" disabled={busy} onClick={() => void signInWithGoogle()}><span className="google-g">G</span> Continue with Google</button>
        {error && <p className="billing-error" role="alert">{error}</p>}
        <p className="signin-terms">TextHalo uses Neon Auth to manage your account and sign-in session.</p>
      </>}
      {!billingApiUrl && selectedName && <p className="signin-config-note">Checkout connection is not configured on this website.</p>}
      <a className="text-link signin-back" href="/pricing/">← Back to plans</a>
    </section>
  </main><BillingFooter /></div>;
}

export function SignInPage() {
  return <SignInContent />;
}

function BillingSuccessContent() {
  const session = authClient.useSession();
  const [credits, setCredits] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function refreshBalance() {
    if (!billingApiUrl || !session.data?.user) return;
    setLoading(true);
    setError("");
    try {
      const token = await getAccessToken();
      if (!token) throw new Error("Please sign in to view your account balance.");
      const response = await fetch(`${billingApiUrl}/v1/account`, { headers: { Authorization: `Bearer ${token}` } });
      if (!response.ok) throw new Error("We couldn’t read your account balance yet.");
      const account = await response.json() as { availableCredits?: number };
      setCredits(typeof account.availableCredits === "number" ? account.availableCredits : null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "We couldn’t read your account balance yet.");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (session.data?.user) void refreshBalance();
  }, [session.data?.user?.id]);

  return <div className="site-shell"><BillingHeader /><main className="billing-success section-wrap">
    <div className="success-orbit"><span>✓</span></div>
    <div className="eyebrow"><span className="eyebrow-line" /> THANK YOU FOR CHOOSING TEXTHALO</div>
    <h1>You’re back.<br /><em>Let’s listen.</em></h1>
    <p className="success-copy">Stripe has returned you to TextHalo. Your hosted speech credits are added after Stripe confirms the payment.</p>
    {session.data?.user ? <div className="balance-card"><span>AVAILABLE CREDITS</span><strong>{credits === null ? (loading ? "Checking…" : "—") : credits.toLocaleString()}</strong>{credits === 0 && <small>Credits may take a moment to appear while Stripe sends its payment confirmation.</small>}<button className="text-link" type="button" disabled={loading} onClick={() => void refreshBalance()}>{loading ? "Refreshing…" : "Refresh balance"}<span className="arrow">→</span></button></div> : <a className="button button-dark button-large success-signin" href="/sign-in/">Sign in to view your account <span className="arrow">→</span></a>}
    {error && <p className="billing-error" role="alert">{error}</p>}
    <div className="success-actions"><a className="button button-dark button-large" href="/pricing/">View plans <span className="arrow">→</span></a><a className="text-link" href="/">Return home <span className="arrow">→</span></a></div>
  </main><BillingFooter /></div>;
}

export function BillingSuccessPage() {
  return <BillingSuccessContent />;
}
