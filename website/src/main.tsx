import React from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { getSeoMetadata, normalizePath, SitePage } from "./pages";
import "./website.css";
import { WebsiteLanguageProvider } from "./i18n";
import { splitLocalizedPath } from "./localization";

const { pathname, language, explicit } = splitLocalizedPath(normalizePath(window.location.pathname));

const root = document.getElementById("root")!;
const app = <React.StrictMode><WebsiteLanguageProvider language={language} explicitEnglish={explicit && language === "en"}><SitePage pathname={pathname} /></WebsiteLanguageProvider></React.StrictMode>;

if (import.meta.env.DEV || document.documentElement.lang !== language) {
  document.title = getSeoMetadata(window.location.pathname).title;
  createRoot(root).render(app);
}
else hydrateRoot(root, app);
