import { resolveLanguage, translate, translateMessage, type Language } from "../../src/localization";
import french from "./locales/fr-CA.json";
import spanish from "./locales/es-419.json";

export const languages: readonly Language[] = ["en", "fr-CA", "es-419"];
export const websiteCatalogs: Record<Exclude<Language, "en">, Record<string, string>> = { "fr-CA": french, "es-419": spanish };
export const languageStorageKey = "texthalo.website.language";

export function splitLocalizedPath(path: string): { language: Language; pathname: string; explicit: boolean } {
  const match = path.match(/^\/(en|fr-CA|es-419)(?=\/|$)/);
  return { language: match ? match[1] as Language : "en", pathname: match ? path.slice(match[0].length) || "/" : path, explicit: Boolean(match) };
}

export function localizedHref(value: string, language: Language, explicitEnglish = false): string {
  if (!value.startsWith("/") || value.startsWith("//")) return value;
  const index = value.search(/[?#]/);
  const path = index < 0 ? value : value.slice(0, index);
  const suffix = index < 0 ? "" : value.slice(index);
  const { pathname } = splitLocalizedPath(path);
  return `${language === "en" && !explicitEnglish ? "" : `/${language}`}${pathname}${suffix}`;
}

export function preferredWebsiteLanguage(path: string, saved: string | null, preferred: readonly string[]): Language {
  const route = splitLocalizedPath(path);
  return route.explicit ? route.language : resolveLanguage(saved ?? "system", preferred);
}

export function websiteTranslate(source: string, language: Language, values: readonly unknown[] = []): string {
  const catalog = language === "en" ? undefined : websiteCatalogs[language];
  if (catalog && Object.prototype.hasOwnProperty.call(catalog, source)) {
    return catalog[source].replace(/\{(\d+)\}/g, (token, index: string) => Number(index) < values.length ? String(values[Number(index)] ?? "") : token);
  }
  if (!values.length && source.endsWith(" | TextHalo")) return `${websiteTranslate(source.slice(0, -11), language)} | TextHalo`;
  return values.length ? translate(source, language, values) : translateMessage(source, language);
}

export function formatPlanFeature(feature: string, plan: { creditsPerPeriod: number; cloneLimit: number }, language: Language): string {
  if (/^[\d\s,.\u00a0\u202f]+ hosted credits each month$/.test(feature)) {
    return websiteTranslate("{0} hosted credits each month", language, [plan.creditsPerPeriod.toLocaleString(language)]);
  }
  if (/^Up to \d+ saved Fish Audio clones$/.test(feature)) {
    return websiteTranslate("Up to {0} saved Fish Audio clones", language, [plan.cloneLimit.toLocaleString(language)]);
  }
  return websiteTranslate(feature, language);
}

export function formatWebsitePlanPrice(price: { unitAmount: number; currency: string; interval: string | null; intervalCount?: number } | null, language: Language): string {
  if (!price) return websiteTranslate("Price unavailable", language);
  const formatter = new Intl.NumberFormat(language, { style: "currency", currency: price.currency });
  const digits = formatter.resolvedOptions().maximumFractionDigits ?? 2;
  const amount = formatter.format(price.unitAmount / (10 ** digits));
  if (!price.interval) return amount;
  const count = price.intervalCount ?? 1;
  const interval = websiteTranslate(`${price.interval}${count > 1 ? "s" : ""}`, language);
  return `${amount} / ${count > 1 ? `${count.toLocaleString(language)} ` : ""}${interval}`;
}
