import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { readFile } from "node:fs/promises";

test("all download buttons link directly to the Apple Silicon DMG; old routes redirect", async () => {
  const vite = await createServer({ configFile: "vite.website.config.ts", server: { middlewareMode: true }, appType: "custom" });
  try {
    const { SitePage, getSitePaths, getSeoMetadata } = await vite.ssrLoadModule("/src/pages.tsx");
    assert.ok(!getSitePaths().includes("/download/"));
    for (const pathname of getSitePaths()) {
      const html = renderToStaticMarkup(React.createElement(SitePage, { pathname }));
        assert.match(html, /href="https:\/\/github.com\/bharat2808\/texthalo\/releases\/latest\/download\/TextHalo-macOS-aarch64.dmg"/);
        assert.doesNotMatch(html, /href="[^"]*x86_64[^"]*"/);
        assert.doesNotMatch(html, /href="\/download\/"/);
        assert.match(html, /Requires Apple Silicon/);
    }
    const url = "https://github.com/bharat2808/texthalo/releases/latest/download/TextHalo-macOS-aarch64.dmg";
    assert.equal(getSeoMetadata("/").schema.downloadUrl, url);
    const redirects = await readFile("website/public/_redirects", "utf8");
    for (const path of ["/download", "/download/"]) assert.ok(redirects.split("\n").includes(`${path} ${url} 302`));
  } finally { await vite.close(); }
});
