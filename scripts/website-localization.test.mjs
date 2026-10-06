import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { createServer } from "vite";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ts from "typescript";

test("website localization preserves backend values, links, and server-rendered languages", async () => {
  const vite = await createServer({ configFile: "vite.website.config.ts", server: { middlewareMode: true, hmr: false, ws: false }, appType: "custom" });
  try {
    const l = await vite.ssrLoadModule("/src/localization.ts");
    const { SitePage, getSitePaths, getSeoMetadata } = await vite.ssrLoadModule("/src/pages.tsx");
    const { WebsiteLanguageProvider } = await vite.ssrLoadModule("/src/i18n.tsx");
    const { blogPosts, stories } = await vite.ssrLoadModule("/src/content.ts");
    assert.equal(l.preferredWebsiteLanguage("/pricing/", null, ["fr-FR"]), "fr-CA");
    assert.equal(l.preferredWebsiteLanguage("/", "en", ["fr-CA"]), "en");
    assert.equal(l.preferredWebsiteLanguage("/", null, ["es-MX"]), "es-419");
    assert.equal(l.preferredWebsiteLanguage("/", null, ["ja-JP"]), "ja");
    assert.equal(l.preferredWebsiteLanguage("/es-419/pricing/", "fr-CA", ["en"]), "es-419");
    assert.equal(l.localizedHref("/fr-CA/desktop-connect/?state=abc#continue", "es-419"), "/es-419/desktop-connect/?state=abc#continue");
    assert.equal(l.localizedHref("/es-419/pricing/", "en"), "/pricing/");
    assert.equal(l.localizedHref("https://storage.texthalo.app/app.dmg", "fr-CA"), "https://storage.texthalo.app/app.dmg");
    assert.equal(l.localizedHref("//other.example/path", "fr-CA"), "//other.example/path");
    assert.equal(l.localizedHref("#voices", "fr-CA"), "#voices");
    const plan = { creditsPerPeriod: 123456, cloneLimit: 2 };
    assert.equal(l.formatPlanFeature("123,456 hosted credits each month", plan, "fr-CA"), `${(123456).toLocaleString("fr-CA")} crédits hébergés par mois`);
    assert.equal(l.formatPlanFeature("Up to 7 saved Fish Audio clones", plan, "es-419"), "Hasta 2 voces clonadas de Fish Audio guardadas");
    assert.equal(l.formatPlanFeature("Future backend benefit", plan, "fr-CA"), "Future backend benefit");
    assert.match(l.formatWebsitePlanPrice({ unitAmount: 2345, currency: "cad", interval: "month" }, "fr-CA"), /23,45.*mois/);
    assert.match(l.formatWebsitePlanPrice({ unitAmount: 4500, currency: "jpy", interval: "year", intervalCount: 2 }, "es-419"), /4,500.*2 años/);
    assert.equal(l.websiteTranslate("constructor", "fr-CA"), "constructor");
    assert.equal(l.websiteTranslate("If there’s an account for {0}, a password reset link is on its way.", "es-419", ["English@example.com"]), "Si hay una cuenta para English@example.com, un enlace de restablecimiento está en camino.");
    assert.deepEqual(Object.keys(l.websiteCatalogs["fr-CA"]).sort(), Object.keys(l.websiteCatalogs["es-419"]).sort());
    for (const [language, catalog] of Object.entries(l.websiteCatalogs)) {
      assert.deepEqual(Object.keys(catalog).sort(), Object.keys(l.websiteCatalogs["fr-CA"]).sort(), `${language}: complete website coverage`);
      for (const [source, target] of Object.entries(catalog)) {
        assert.ok(target.trim(), `${language}: ${source}`);
        assert.deepEqual(target.match(/\{\d+\}/g)?.sort() ?? [], source.match(/\{\d+\}/g)?.sort() ?? [], `${language}: parameters in ${source}`);
      }
      for (const page of [...blogPosts, ...stories]) {
        for (const source of [page.title, page.description, page.category, page.readTime, ...page.blocks.flatMap(block => block.type === "list" ? block.items : [block.text])]) {
          // “Privacy” is also the conventional Italian label; equal text is not a missing translation.
          if (language === "it" && source === "Privacy") assert.equal(catalog[source], "Privacy");
          else assert.notEqual(l.websiteTranslate(source, language), source, `${language}: editorial ${source}`);
        }
      }
      for (const pathname of getSitePaths()) {
        const html = renderToStaticMarkup(React.createElement(WebsiteLanguageProvider, { language }, React.createElement(SitePage, { pathname })));
        assert.match(html, /href="https:\/\/storage.texthalo.app\/TextHalo-macOS-aarch64.dmg"/);
        assert.doesNotMatch(html, /Requires Apple Silicon|Free local features|Main navigation/);
        assert.match(html, new RegExp(`href="/${language}/`));
        const metadata = getSeoMetadata(l.localizedHref(pathname, language));
        assert.equal(metadata.path, l.localizedHref(pathname, language));
        assert.notEqual(metadata.title, getSeoMetadata(pathname).title);
        assert.notEqual(metadata.description, getSeoMetadata(pathname).description);
        assert.equal(metadata.schema.inLanguage, language);
      }
      // Catch missing translations when a new authored label is added to a component.
      for (const file of ["website/src/website.tsx", "website/src/pages.tsx", "website/src/billing.tsx", "website/src/ThirdPartyLicenses.tsx"]) {
        const ast = ts.createSourceFile(file, await readFile(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
        const visit = node => {
          if (ts.isCallExpression(node) && ["i18n.t", "t"].includes(node.expression.getText(ast)) && ts.isStringLiteral(node.arguments[0])) {
            const source = node.arguments[0].text;
            assert.ok(Object.prototype.hasOwnProperty.call(catalog, source) || l.websiteTranslate(source, language) !== source, `${language}: missing UI label ${source}`);
          }
          ts.forEachChild(node, visit);
        };
        visit(ast);
      }
    }
  } finally { await vite.close(); }
});
