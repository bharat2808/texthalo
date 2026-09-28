import { createAuthClient, createInternalNeonAuth } from "@neondatabase/auth";
import { BetterAuthReactAdapter } from "@neondatabase/auth/react/adapters";

const neonAuthUrl = import.meta.env.VITE_NEON_AUTH_URL || "https://ep-hidden-moon-b4ppme87.neonauth.c-6.us-east-2.aws.neon.tech/neondb/auth";

export const authClient = createAuthClient(neonAuthUrl, {
  adapter: BetterAuthReactAdapter(),
});
const tokenAuth = createInternalNeonAuth(neonAuthUrl);

export const billingApiUrl = (import.meta.env.VITE_TEXTHALO_API_URL || (import.meta.env.DEV ? "http://localhost:8788" : "")).replace(/\/$/, "");
const ngrokRequestHeaders = ((): Record<string, string> => {
  if (!billingApiUrl) return {};
  try {
    return new URL(billingApiUrl).hostname.endsWith(".ngrok-free.dev")
      ? { "ngrok-skip-browser-warning": "1" }
      : {};
  } catch {
    return {};
  }
})();

export function billingApiHeaders(headers: Record<string, string> = {}): Record<string, string> {
  return { ...ngrokRequestHeaders, ...headers };
}

export async function getAccessToken(): Promise<string | null> {
  return tokenAuth.getJWTToken();
}

export function safeInternalReturnTo(value: string | null | undefined): string | null {
  if (!value || !value.startsWith("/") || value.startsWith("//")) return null;
  const destination = new URL(value, window.location.origin);
  if (destination.origin !== window.location.origin || destination.pathname === "/sign-in/") return null;
  return `${destination.pathname}${destination.search}${destination.hash}`;
}

export async function promptSignIn(options: { planId?: "plus" | "creator"; topupId?: string; returnTo?: string } = {}): Promise<void> {
  try {
    await authClient.signOut();
  } catch {
    // Expired sessions can reject sign-out at the auth service; continue to sign-in anyway.
  }
  const query = new URLSearchParams({ reason: "session-expired" });
  if (options.planId) query.set("plan", options.planId);
  if (options.topupId) query.set("topup", options.topupId);
  const returnTo = safeInternalReturnTo(options.returnTo);
  if (returnTo) query.set("returnTo", returnTo);
  window.location.assign(`/sign-in/?${query.toString()}`);
}

export async function startCheckout(planId: "plus" | "creator"): Promise<void> {
  if (!billingApiUrl) throw new Error("Checkout is not connected on this website yet.");
  const token = await getAccessToken();
  if (!token) {
    await promptSignIn({ planId });
    return;
  }

  const response = await fetch(`${billingApiUrl}/v1/billing/checkout`, {
    method: "POST",
    headers: billingApiHeaders({
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    }),
    body: JSON.stringify({ planId }),
  });
  if (response.status === 401) {
    await promptSignIn({ planId });
    return;
  }
  const result = await response.json().catch(() => null) as { url?: string; error?: string; message?: string } | null;
  if (!response.ok || !result?.url) {
    const messages: Record<string, string> = {
      billing_unavailable: "Checkout is temporarily unavailable. Please try again soon.",
      checkout_unavailable: "We couldn’t open your Stripe checkout or subscription settings. Please try again in a moment.",
      unknown_plan: "This plan is not configured for checkout yet.",
      multiple_subscriptions: "We found more than one active TextHalo subscription. Please contact support so we can reconcile your plans.",
      subscription_needs_attention: "Please update your payment method or resolve the existing subscription before changing plans.",
      unsupported_subscription_plan: "We couldn’t match your current subscription to a TextHalo plan. Please contact support.",
      checkout_in_progress: "A checkout is already open or still syncing. Finish it, or refresh in a moment before choosing another plan.",
    };
    const message = result?.error ? messages[result.error] ?? "We couldn’t start checkout. Please try again." : "We couldn’t start checkout. Please try again.";
    throw new Error(message);
  }
  window.location.assign(result.url);
}

export async function startCreditTopupCheckout(packId: string): Promise<void> {
  if (!billingApiUrl) throw new Error("Credit purchases are not connected on this website yet.");
  const token = await getAccessToken();
  if (!token) {
    await promptSignIn({ topupId: packId });
    return;
  }
  const response = await fetch(`${billingApiUrl}/v1/billing/topup`, {
    method: "POST",
    headers: billingApiHeaders({ Authorization: `Bearer ${token}`, "Content-Type": "application/json" }),
    body: JSON.stringify({ packId }),
  });
  if (response.status === 401) {
    await promptSignIn({ topupId: packId });
    return;
  }
  const result = await response.json().catch(() => null) as { url?: string; error?: string } | null;
  if (!response.ok || !result?.url) {
    const messages: Record<string, string> = {
      billing_unavailable: "Credit purchases are temporarily unavailable. Please try again soon.",
      unknown_topup: "That credit pack is no longer available. Refresh the page and try again.",
      topup_unavailable: "Credit purchases are not configured yet.",
      topup_checkout_unavailable: "We couldn’t open Stripe checkout. Please try again in a moment.",
    };
    throw new Error(result?.error ? messages[result.error] ?? "We couldn’t start the credit purchase. Please try again." : "We couldn’t start the credit purchase. Please try again.");
  }
  window.location.assign(result.url);
}
