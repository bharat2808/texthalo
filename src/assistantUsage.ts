export type AssistantUsage = { billingStatus?: string; reservedCredits?: number; chargedCredits?: number; refundedCredits?: number };

export function assistantUsageLabel(usage?: AssistantUsage): string {
  if (!usage) return "Uses credits";
  if (usage.billingStatus === "settled" || usage.billingStatus === "released") {
    return typeof usage.chargedCredits === "number"
      ? `${usage.chargedCredits.toLocaleString()} credits for generation`
      : "Checking generation usage…";
  }
  if (usage.billingStatus === "reserved" && typeof usage.reservedCredits === "number") {
    return `${usage.reservedCredits.toLocaleString()} credits reserved`;
  }
  return "Checking generation usage…";
}
