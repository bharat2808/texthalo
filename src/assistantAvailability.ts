import { t } from "./i18n";
export function localAssistantReason(model?: string): string {
  if (!model) return t("Checking Apple Intelligence…");
  if (model === "available") return t("Apple Intelligence · no credits");
  if (model.includes("Requires macOS")) return t("On-device Assistant requires macOS 26 or later and a Mac that supports Apple Intelligence.");
  if (model.includes("appleIntelligenceNotEnabled")) return t("Turn on Apple Intelligence in System Settings to use on-device Assistant.");
  if (model.includes("modelNotReady")) return t("Apple Intelligence is still downloading or preparing. Try again when it is ready.");
  if (model.includes("deviceNotEligible")) return t("This Mac does not support Apple Intelligence. Use Cloud Assistant instead.");
  return t("Apple Intelligence is unavailable on this Mac. Use Cloud Assistant instead.");
}
