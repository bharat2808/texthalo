import { currentLanguage, t } from "./i18n";
export type AssistantUsage = { billingStatus?: string; reservedCredits?: number; chargedCredits?: number; refundedCredits?: number; outputLimited?: boolean; maxOutputTokens?: number };

export function assistantUsageLabel(usage?: AssistantUsage): string {
  if (!usage) return t("Uses credits");
  if (usage.billingStatus === "settled" || usage.billingStatus === "released") {
    return typeof usage.chargedCredits === "number"
      ? t("{0} credits for generation{1}", [usage.chargedCredits.toLocaleString(currentLanguage()), usage.outputLimited ? t(" · shorter answer limit") : ""])
      : t("Checking generation usage…");
  }
  if (usage.billingStatus === "reserved" && typeof usage.reservedCredits === "number") {
    return t("{0} credits reserved{1}", [usage.reservedCredits.toLocaleString(currentLanguage()), usage.outputLimited ? t(" · answer length limited by credit budget") : ""]);
  }
  return t("Checking generation usage…");
}
