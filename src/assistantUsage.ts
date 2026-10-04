export type AssistantUsage = { billingStatus?: string; reservedCredits?: number; chargedCredits?: number; refundedCredits?: number; outputLimited?: boolean; maxOutputTokens?: number };

export function assistantUsageLabel(usage?: AssistantUsage): string {
  if (!usage) return "Uses credits";
  if (usage.billingStatus === "settled" || usage.billingStatus === "released") {
    return typeof usage.chargedCredits === "number"
      ? `${usage.chargedCredits.toLocaleString()} credits for generation${usage.outputLimited ? " · shorter answer limit" : ""}`
      : "Checking generation usage…";
  }
  if (usage.billingStatus === "reserved" && typeof usage.reservedCredits === "number") {
    return `${usage.reservedCredits.toLocaleString()} credits reserved${usage.outputLimited ? " · answer length limited by credit budget" : ""}`;
  }
  return "Checking generation usage…";
}
