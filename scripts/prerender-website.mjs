import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import React from "react";
import { renderToString } from "react-dom/server";
import { createServer } from "vite";

const siteUrl = "https://texthalo.app";
const outputDirectory = resolve("dist-website");
const vite = await createServer({
  configFile: resolve("vite.website.config.ts"),
  server: { middlewareMode: true },
  appType: "custom",
});

try {
  const { DEMO_VIDEO } = await vite.ssrLoadModule("/src/demo.ts");
  const { SitePage, getSeoMetadata, getSitePaths } = await vite.ssrLoadModule("/src/pages.tsx");
  const shell = await readFile(resolve(outputDirectory, "index.html"), "utf8");
  const { WebsiteLanguageProvider } = await vite.ssrLoadModule("/src/i18n.tsx");
  const { languages, localizedHref, splitLocalizedPath, websiteTranslate } = await vite.ssrLoadModule("/src/localization.ts");
  const baseRoutes = getSitePaths();
  const routes = baseRoutes.flatMap(path => [...languages.map(language => localizedHref(path, language)), localizedHref(path, "en", true)]);
  const renderPage = pathname => {
    const route = splitLocalizedPath(pathname);
    return renderToString(React.createElement(WebsiteLanguageProvider, { language: route.language, explicitEnglish: route.explicit && route.language === "en" }, React.createElement(SitePage, { pathname: route.pathname })));
  };

  for (const pathname of routes) {
    const metadata = getSeoMetadata(pathname);
    const route = splitLocalizedPath(pathname);
    const image = `${siteUrl}${DEMO_VIDEO.poster}`;
    const headTags = [
      `<title>${escapeHtml(metadata.title)}</title>`,
      `<meta name="description" content="${escapeHtml(metadata.description)}" />`,
      `<meta name="robots" content="${["/sign-in/", "/desktop-connect/", "/reset-password/", "/account/billing/success/"].includes(route.pathname) ? "noindex,follow" : "index,follow,max-image-preview:large"}" />`,
      `<link rel="canonical" href="${siteUrl}${metadata.path}" />`,
      ...languages.map(language => `<link rel="alternate" hreflang="${language}" href="${siteUrl}${localizedHref(route.pathname, language)}" />`),
      `<link rel="alternate" hreflang="x-default" href="${siteUrl}${route.pathname}" />`,
      `<meta property="og:locale" content="${({ en: "en_US", "fr-CA": "fr_CA", "es-419": "es_MX" })[route.language]}" />`,
      `<meta property="og:type" content="${metadata.type}" />`,
      `<meta property="og:site_name" content="TextHalo" />`,
      `<meta property="og:title" content="${escapeHtml(metadata.title)}" />`,
      `<meta property="og:description" content="${escapeHtml(metadata.description)}" />`,
      `<meta property="og:url" content="${siteUrl}${metadata.path}" />`,
      `<meta property="og:image" content="${image}" />`,
      `<meta property="og:image:alt" content="${escapeHtml(websiteTranslate("TextHalo animated demo of Listen, Dictate, and Assistant", route.language))}" />`,
      ...(metadata.video ? [
        `<meta property="og:video" content="${metadata.video.contentUrl}" />`,
        `<meta property="og:video:secure_url" content="${metadata.video.contentUrl}" />`,
        `<meta property="og:video:type" content="video/mp4" />`,
        `<meta property="og:video:width" content="${metadata.video.width}" />`,
        `<meta property="og:video:height" content="${metadata.video.height}" />`,
      ] : []),
      `<meta name="twitter:card" content="summary_large_image" />`,
      `<meta name="twitter:title" content="${escapeHtml(metadata.title)}" />`,
      `<meta name="twitter:description" content="${escapeHtml(metadata.description)}" />`,
      `<meta name="twitter:image" content="${image}" />`,
      `<script type="application/ld+json">${JSON.stringify(metadata.schema).replaceAll("<", "\\u003c")}</script>`,
    ].join("\n    ");
    const html = shell
      .replace(/<html lang="[^"]*"/, `<html lang="${route.language}"`)
      .replace(/<title>[\s\S]*?<\/title>/, "")
      .replace(/<meta name="description"[^>]*\/?\s*>/, "")
      .replace(/<link rel="canonical"[^>]*\/?\s*>/, "")
      .replace(/<meta (?:property="og:[^"]+"|name="twitter:[^"]+")[^>]*\/?\s*>/g, "")
      .replace(/<div id="root">[\s\S]*?<\/div>/, `<div id="root">${renderPage(pathname)}</div>`)
      .replace("</head>", `    ${headTags}\n  </head>`);
    const filePath = pathname === "/"
      ? resolve(outputDirectory, "index.html")
      : resolve(outputDirectory, pathname.slice(1), "index.html");
    await mkdir(resolve(filePath, ".."), { recursive: true });
    await writeFile(filePath, html);
    console.log(`Pre-rendered ${pathname}`);
  }

  const notFoundHtml = shell
    .replace(/<title>[\s\S]*?<\/title>/, "")
    .replace(/<meta name="description"[^>]*\/?\s*>/, "")
    .replace(/<link rel="canonical"[^>]*\/?\s*>/, "")
    .replace(/<meta (?:property="og:[^"]+"|name="twitter:[^"]+")[^>]*\/?\s*>/g, "")
    .replace(/<div id="root">[\s\S]*?<\/div>/, `<div id="root">${renderPage("/not-found/")}</div>`)
    .replace("</head>", `    <title>Page not found | TextHalo</title>\n    <meta name="robots" content="noindex,follow" />\n    <meta name="description" content="This TextHalo page could not be found." />\n  </head>`);
  await writeFile(resolve(outputDirectory, "404.html"), notFoundHtml);
  for (const language of languages) {
    const pathname = localizedHref("/not-found/", language, language === "en");
    const metadata = getSeoMetadata(pathname);
    const html = shell
      .replace(/<html lang="[^"]*"/, `<html lang="${language}"`)
      .replace(/<title>[\s\S]*?<\/title>/, "")
      .replace(/<meta name="description"[^>]*\/?\s*>/, "")
      .replace(/<link rel="canonical"[^>]*\/?\s*>/, "")
      .replace(/<meta (?:property="og:[^"]+"|name="twitter:[^"]+")[^>]*\/?\s*>/g, "")
      .replace(/<div id="root">[\s\S]*?<\/div>/, `<div id="root">${renderPage(pathname)}</div>`)
      .replace("</head>", `    <title>${escapeHtml(metadata.title)}</title>\n    <meta name="robots" content="noindex,follow" />\n    <meta name="description" content="${escapeHtml(metadata.description)}" />\n  </head>`);
    await writeFile(resolve(outputDirectory, language, "404.html"), html);
  }


  const today = new Date().toISOString().slice(0, 10);
  const urlEntries = routes.filter(path => !path.startsWith("/en/")).map((pathname) => {
    const metadata = getSeoMetadata(pathname);
    const route = splitLocalizedPath(pathname);
    const alternates = languages.map(language => `<xhtml:link rel="alternate" hreflang="${language}" href="${siteUrl}${localizedHref(route.pathname, language)}" />`).join("");
    const lastmod = metadata.modified ?? metadata.published ?? today;
    const videoEntry = metadata.video ? `\n    <video:video>
      <video:thumbnail_loc>${xmlEscape(metadata.video.thumbnailUrl)}</video:thumbnail_loc>
      <video:title>${xmlEscape(metadata.video.name)}</video:title>
      <video:description>${xmlEscape(metadata.video.description)}</video:description>
      <video:content_loc>${xmlEscape(metadata.video.contentUrl)}</video:content_loc>
      <video:duration>${DEMO_VIDEO.durationSeconds}</video:duration>
      <video:publication_date>${metadata.video.uploadDate}</video:publication_date>
    </video:video>` : "";
    return `  <url><loc>${siteUrl}${pathname}</loc><lastmod>${lastmod}</lastmod>${alternates}${videoEntry}</url>`;
  }).join("\n");
  await writeFile(resolve(outputDirectory, "robots.txt"), `User-agent: *\nAllow: /\n\nSitemap: ${siteUrl}/sitemap.xml\n`);
  await writeFile(resolve(outputDirectory, "sitemap.xml"), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:video="http://www.google.com/schemas/sitemap-video/1.1" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n${urlEntries}\n</urlset>\n`);
} finally {
  await vite.close();
}

function escapeHtml(value) {
  return value.replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function xmlEscape(value) {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&apos;");
}
