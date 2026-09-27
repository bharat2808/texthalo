import { createAuthClient } from "@neondatabase/auth";
import { BetterAuthReactAdapter } from "@neondatabase/auth/react/adapters";

const neonAuthUrl = import.meta.env.VITE_NEON_AUTH_URL || "https://ep-hidden-moon-b4ppme87.neonauth.c-6.us-east-2.aws.neon.tech/neondb/auth";

export const authClient = createAuthClient(neonAuthUrl, {
  adapter: BetterAuthReactAdapter(),
});

export const billingApiUrl = (import.meta.env.VITE_TEXTHALO_API_URL || (import.meta.env.DEV ? "http://localhost:8788" : "")).replace(/\/$/, "");

export async function getAccessToken(): Promise<string | null> {
  const result = await authClient.token();
  if (result.error) return null;
  return result.data?.token ?? null;
}

export async function startCheckout(planId: "plus" | "creator"): Promise<void> {
  if (!billingApiUrl) throw new Error("Checkout is not connected on this website yet.");
  const token = await getAccessToken();
  if (!token) throw new Error("Your sign-in session expired. Please sign in again.");

  const response = await fetch(`${billingApiUrl}/v1/billing/checkout`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ planId }),
  });
  const result = await response.json().catch(() => null) as { url?: string; error?: string; message?: string } | null;
  if (!response.ok || !result?.url) {
    const messages: Record<string, string> = {
      billing_unavailable: "Checkout is temporarily unavailable. Please try again soon.",
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
