import { createContext, useContext, useEffect, type ReactNode } from "react";
import { languageNames, languages, type Language } from "../../src/localization";
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
  const shortName = language === "zh-Hans" ? "简" : language === "zh-Hant" ? "繁" : language.split("-")[0].toUpperCase();
  return <label className="website-language-picker" title={languageNames[language]}>
    <span className="website-language-display" aria-hidden="true">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="12" cy="12" r="9" /><ellipse cx="12" cy="12" rx="4" ry="9" /><path d="M3 12h18" /></svg>
      <span>{shortName}</span>
      <svg className="website-language-chevron" width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="m3 4.5 3 3 3-3" /></svg>
    </span>
    <select aria-label={t("Language")} value={language} onChange={event => {
    const selected = event.target.value as Language;
    try { localStorage.setItem(languageStorageKey, selected); } catch { /* The URL still preserves the choice. */ }
    window.location.assign(localizedHref(`${window.location.pathname}${window.location.search}${window.location.hash}`, selected, selected === "en"));
  }}>{languages.map(language => <option key={language} value={language}>{languageNames[language]}</option>)}</select></label>;
}
