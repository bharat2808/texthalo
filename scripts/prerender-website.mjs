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
  const { SitePage, getSeoMetadata, getSitePaths } = await vite.ssrLoadModule("/src/pages.tsx");
  const shell = await readFile(resolve(outputDirectory, "index.html"), "utf8");
  const routes = getSitePaths();

  for (const pathname of routes) {
    const metadata = getSeoMetadata(pathname);
    const image = `${siteUrl}/videos/texthalo-product-demo-poster.png`;
    const headTags = [
      `<title>${escapeHtml(metadata.title)}</title>`,
      `<meta name="description" content="${escapeHtml(metadata.description)}" />`,
      `<meta name="robots" content="${["/sign-in/", "/desktop-connect/", "/reset-password/", "/account/billing/success/"].includes(metadata.path) ? "noindex,follow" : "index,follow,max-image-preview:large"}" />`,
      `<link rel="canonical" href="${siteUrl}${metadata.path}" />`,
      `<meta property="og:type" content="${metadata.type}" />`,
      `<meta property="og:site_name" content="TextHalo" />`,
      `<meta property="og:title" content="${escapeHtml(metadata.title)}" />`,
      `<meta property="og:description" content="${escapeHtml(metadata.description)}" />`,
      `<meta property="og:url" content="${siteUrl}${metadata.path}" />`,
      `<meta property="og:image" content="${image}" />`,
      `<meta property="og:image:alt" content="TextHalo app for listening to selected text on Mac" />`,
      ...(metadata.video ? [
        `<meta property="og:video" content="${metadata.video.contentUrl}" />`,
        `<meta property="og:video:secure_url" content="${metadata.video.contentUrl}" />`,
        `<meta property="og:video:type" content="video/mp4" />`,
        `<meta property="og:video:width" content="960" />`,
        `<meta property="og:video:height" content="540" />`,
      ] : []),
      `<meta name="twitter:card" content="summary_large_image" />`,
      `<meta name="twitter:title" content="${escapeHtml(metadata.title)}" />`,
      `<meta name="twitter:description" content="${escapeHtml(metadata.description)}" />`,
      `<meta name="twitter:image" content="${image}" />`,
      `<script type="application/ld+json">${JSON.stringify(metadata.schema).replaceAll("<", "\\u003c")}</script>`,
    ].join("\n    ");
    const html = shell
      .replace(/<title>[\s\S]*?<\/title>/, "")
      .replace(/<meta name="description"[^>]*\/?\s*>/, "")
      .replace(/<link rel="canonical"[^>]*\/?\s*>/, "")
      .replace(/<meta (?:property="og:[^"]+"|name="twitter:[^"]+")[^>]*\/?\s*>/g, "")
      .replace(/<div id="root">[\s\S]*?<\/div>/, `<div id="root">${renderToString(React.createElement(SitePage, { pathname }))}</div>`)
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
    .replace(/<div id="root">[\s\S]*?<\/div>/, `<div id="root">${renderToString(React.createElement(SitePage, { pathname: "/not-found/" }))}</div>`)
    .replace("</head>", `    <title>Page not found | TextHalo</title>\n    <meta name="robots" content="noindex,follow" />\n    <meta name="description" content="This TextHalo page could not be found." />\n  </head>`);
  await writeFile(resolve(outputDirectory, "404.html"), notFoundHtml);

  const today = new Date().toISOString().slice(0, 10);
  const urlEntries = routes.map((pathname) => {
    const metadata = getSeoMetadata(pathname);
    const lastmod = metadata.modified ?? metadata.published ?? today;
    const videoEntry = metadata.video ? `\n    <video:video>
      <video:thumbnail_loc>${xmlEscape(metadata.video.thumbnailUrl)}</video:thumbnail_loc>
      <video:title>${xmlEscape(metadata.video.name)}</video:title>
      <video:description>${xmlEscape(metadata.video.description)}</video:description>
      <video:content_loc>${xmlEscape(metadata.video.contentUrl)}</video:content_loc>
      <video:duration>15</video:duration>
      <video:publication_date>${metadata.video.uploadDate}</video:publication_date>
    </video:video>` : "";
    return `  <url><loc>${siteUrl}${pathname}</loc><lastmod>${lastmod}</lastmod>${videoEntry}</url>`;
  }).join("\n");
  await writeFile(resolve(outputDirectory, "robots.txt"), `User-agent: *\nAllow: /\n\nSitemap: ${siteUrl}/sitemap.xml\n`);
  await writeFile(resolve(outputDirectory, "sitemap.xml"), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:video="http://www.google.com/schemas/sitemap-video/1.1">\n${urlEntries}\n</urlset>\n`);
} finally {
  await vite.close();
}

function escapeHtml(value) {
  return value.replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function xmlEscape(value) {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&apos;");
}
