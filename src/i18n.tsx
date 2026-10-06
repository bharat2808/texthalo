import { useEffect, useSyncExternalStore, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { resolveLanguage, translate, translateMessage, type Language, type LanguagePreference } from "./localization";

export type LocalizationState = { preference: LanguagePreference; language: Language };
let language: Language = resolveLanguage("system", typeof navigator === "undefined" ? [] : navigator.languages);
const subscribers = new Set<() => void>();
export function applyLocalization(state: LocalizationState) {
  if (state.language !== "en" && state.language !== "fr-CA" && state.language !== "es-419") return;
  language = state.language;
  if (typeof document !== "undefined") document.documentElement.lang = language;
  subscribers.forEach(listener => listener());
}
export function useLanguage(): Language {
  return useSyncExternalStore(callback => {
    subscribers.add(callback);
    return () => { subscribers.delete(callback); };
  }, () => language, () => "en");
}
export function sourceMessage(source: string, values: readonly unknown[] = []): string { return translate(source, "en", values); }
export function t(source: string, values: readonly unknown[] = []): string {
  return translate(source, language, values);
}
export function message(source: string): string { return translateMessage(source, language); }
export function currentLanguage(): Language { return language; }

export function LanguageProvider({ children }: { children: ReactNode }) {
  useEffect(() => {
    let disposed = false;
    let received = false;
    let unsubscribe: (() => void) | undefined;
    document.documentElement.lang = language;
    void listen<LocalizationState>("texthalo:language", event => {
      received = true;
      if (!disposed) applyLocalization(event.payload);
    }).then(off => {
      if (disposed) { off(); return; }
      unsubscribe = off;
      return invoke<LocalizationState>("localization_state").then(state => {
        if (!disposed && !received) applyLocalization(state);
      });
    }).catch(() => { /* Browser previews use navigator languages; native snapshots retry on remount. */ });
    return () => { disposed = true; unsubscribe?.(); };
  }, []);
  return children;
}
