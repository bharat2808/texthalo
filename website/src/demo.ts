import { languageNames, type Language } from "../../src/localization";
import { websiteTranslate } from "./localization";

const BASE_DEMO_VIDEO = {
  width: 1280,
  height: 720,
  durationSeconds: 30,
  duration: "PT30S",
  published: "2026-10-04T00:00:00-04:00",
  title: "TextHalo for Mac: Listen, Dictate, and Assistant",
  description: "A 30-second animated walkthrough of TextHalo: listen to selected text, dictate into the current app, and ask Assistant to edit a draft. Includes example interactions and narration.",
};

export function getDemoVideo(language: Language) {
  const basename = `/videos/texthalo-workflow-demo${language === "en" ? "" : `-${language}`}`;
  return {
    ...BASE_DEMO_VIDEO,
    published: language === "en" ? BASE_DEMO_VIDEO.published : "2026-10-06T00:00:00-04:00",
    src: `${basename}.mp4`,
    captions: `${basename}.vtt`,
    poster: `${basename}-poster.png`,
    language,
    captionLabel: languageNames[language],
    title: websiteTranslate(BASE_DEMO_VIDEO.title, language),
    description: websiteTranslate(BASE_DEMO_VIDEO.description, language),
  };
}
export const DEMO_VIDEO = getDemoVideo("en");
