export type RecognitionLanguage = { code: string; name: string };

export function groupRecognitionLanguages(languages: RecognitionLanguage[], displayLocale = "en"): { name: string; languages: RecognitionLanguage[] }[] {
  const names = new Intl.DisplayNames([displayLocale], { type: "language" });
  const groups = new Map<string, RecognitionLanguage[]>();
  for (const language of languages) {
    const base = language.code.split(/[-_]/)[0];
    const group = groups.get(base) ?? [];
    group.push(language); groups.set(base, group);
  }
  return [...groups].map(([code, languages]) => ({ name: names.of(code) ?? code, languages: [...languages].sort((a, b) => a.name.localeCompare(b.name)) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

