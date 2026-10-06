import { createContext, useContext, useEffect, type ReactNode } from "react";
import type { Language } from "../../src/localization";
import { languageStorageKey, localizedHref, preferredWebsiteLanguage, websiteTranslate } from "./localization";

const LanguageContext = createContext({ language: "en" as Language, explicitEnglish: false });

export function WebsiteLanguageProvider({ language, explicitEnglish = false, children }: { language: Language; explicitEnglish?: boolean; children: ReactNode }) {
  useEffect(() => {
    document.documentElement.lang = language;
    let saved: string | null = null;
    try { saved = localStorage.getItem(languageStorageKey); } catch { /* Storage can be disabled. */ }
    const preferred = preferredWebsiteLanguage(window.location.pathname, saved, navigator.languages);
    if (preferred !== language) window.location.replace(localizedHref(`${window.location.pathname}${window.location.search}${window.location.hash}`, preferred));
  }, [language]);
  return <LanguageContext.Provider value={{ language, explicitEnglish }}>{children}</LanguageContext.Provider>;
}

export function useWebsiteI18n() {
  const { language, explicitEnglish } = useContext(LanguageContext);
  return {
    language,
    t: (source: string, values: readonly unknown[] = []) => websiteTranslate(source, language, values),
    href: (value: string) => localizedHref(value, language, explicitEnglish),
  };
}

export function LanguagePicker() {
  const { language, t } = useWebsiteI18n();
  return <label className="website-language-picker"><span className="sr-only">{t("Language")}</span><select aria-label={t("Language")} value={language} onChange={event => {
    const selected = event.target.value as Language;
    try { localStorage.setItem(languageStorageKey, selected); } catch { /* The URL still preserves the choice. */ }
    window.location.assign(localizedHref(`${window.location.pathname}${window.location.search}${window.location.hash}`, selected, selected === "en"));
  }}><option value="en">English</option><option value="fr-CA">Français (Canada)</option><option value="es-419">Español (Latinoamérica)</option></select></label>;
}
