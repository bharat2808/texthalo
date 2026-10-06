import french from "./locales/fr-CA.json";
import spanish from "./locales/es-419.json";
import ptBR from "./locales/pt-BR.json";
import it from "./locales/it.json";
import ja from "./locales/ja.json";
import zhHans from "./locales/zh-Hans.json";
import zhHant from "./locales/zh-Hant.json";

export const languageNames = {
  en: "English", "fr-CA": "Français (Canada)", "es-419": "Español (Latinoamérica)",
  "pt-BR": "Português (Brasil)", it: "Italiano", ja: "日本語",
  "zh-Hans": "简体中文", "zh-Hant": "繁體中文",
} as const;
export type Language = keyof typeof languageNames;
export const languages: readonly Language[] = Object.keys(languageNames) as Language[];
export function isLanguage(value: string): value is Language {
  return Object.prototype.hasOwnProperty.call(languageNames, value);
}

export type LanguagePreference = Language | "system";
export const catalogs: Record<Exclude<Language, "en">, Record<string, string>> = {
  "fr-CA": french,
  "es-419": spanish,
  "pt-BR": ptBR,
  it,
  ja,
  "zh-Hans": zhHans,
  "zh-Hant": zhHant,
};

export function resolveLanguage(preference: string | undefined, preferred: readonly string[]): Language {
  if (preference && isLanguage(preference)) return preference;
  for (const locale of preferred) {
    const parts = locale.toLowerCase().split(/[-_]/);
    const language = parts[0];
    if (language === "en") return "en";
    if (language === "fr") return "fr-CA";
    if (language === "es") return "es-419";
    if (language === "pt") return "pt-BR";
    if (language === "it") return "it";
    if (language === "ja") return "ja";
    if (language === "zh") {
      if (parts.includes("hant")) return "zh-Hant";
      if (parts.includes("hans")) return "zh-Hans";
      return parts.some(part => ["tw", "hk", "mo"].includes(part)) ? "zh-Hant" : "zh-Hans";
    }
  }
  return "en";
}

export function translate(source: string, language: Language, values: readonly unknown[] = []): string {
  const value = language === "en" ? source : catalogs[language][source];
  const text = typeof value === "string" ? value : source;
  return text.replace(/\{(\d+)\}/g, (token, index: string) => Number(index) < values.length ? String(values[Number(index)] ?? "") : token);
}

// Native messages retain their English wire text for error routing and audit history.
// Translate only at presentation boundaries; interpolation values stay verbatim.
const messagePatterns = Object.keys(french).filter(key => /\{\d+\}/.test(key)).map(source => {
  const indices: number[] = [];
  let cursor = 0;
  let pattern = "^";
  const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  for (const match of source.matchAll(/\{(\d+)\}/g)) {
    pattern += escape(source.slice(cursor, match.index)) + "([\\s\\S]*?)";
    indices.push(Number(match[1]));
    cursor = match.index! + match[0].length;
  }
  pattern += escape(source.slice(cursor)) + "$";
  return { source, pattern: new RegExp(pattern), indices };
});
export function translateMessage(source: string, language: Language): string {
  if (language === "en" || Object.prototype.hasOwnProperty.call(catalogs[language], source)) return translate(source, language);
  for (const { source: template, pattern, indices } of messagePatterns) {
    const match = pattern.exec(source);
    if (match) {
      const values: string[] = [];
      indices.forEach((index, position) => { values[index] = match[position + 1]; });
      // These fields are authored labels/messages, unlike voice names and diagnostics.
      if (template === "“{0}” was uploaded and saved. {1}") values[1] = translateMessage(values[1], language);
      if (template.startsWith("Already used by {0}.")) values[0] = translate(values[0], language);
      return translate(template, language, values);
    }
  }
  return source;
}

const phaseLabels: Record<string, string> = {
  capturing_screen: "Capturing screen", transcript: "Transcript",
  started: "Started", recording: "Recording", processing: "Processing",
  streaming: "Generating", result: "Result", error: "Error",
  canceled: "Canceled", interrupted: "Interrupted", speaking: "Speaking", idle: "Idle",
};
export function translatePhase(phase: string, language: Language): string {
  return Object.prototype.hasOwnProperty.call(phaseLabels, phase) ? translate(phaseLabels[phase], language) : phase;
}
