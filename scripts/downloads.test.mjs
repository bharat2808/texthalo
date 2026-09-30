import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

test("every site route offers the Apple Silicon download page without Intel links", async () => {
  const vite = await createServer({ configFile: "vite.website.config.ts", server: { middlewareMode: true }, appType: "custom" });
  try {
    const { SitePage, getSitePaths, getSeoMetadata } = await vite.ssrLoadModule("/src/pages.tsx");
    assert.ok(getSitePaths().includes("/download/"));
    for (const pathname of getSitePaths()) {
      const html = renderToStaticMarkup(React.createElement(SitePage, { pathname }));
      if (pathname === "/download/") {
        assert.match(html, /href="https:\/\/github.com\/bharat2808\/texthalo\/releases\/latest\/download\/TextHalo-macOS-aarch64.dmg"/);
        assert.doesNotMatch(html, /href="[^"]*x86_64[^"]*"/);
        assert.match(html, /Intel Macs are not supported/);
        assert.match(html, /About This Mac/);
      } else {
        assert.doesNotMatch(html, /href="[^"]+\.dmg"/, pathname);
        assert.match(html, /href="\/download\/"/, pathname);
      }
    }
    assert.equal(getSeoMetadata("/").schema.downloadUrl, "https://texthalo.app/download/");
  } finally { await vite.close(); }
});
