import assert from 'node:assert/strict';
import test from 'node:test';
import { access, readFile } from 'node:fs/promises';
import { createServer } from 'vite';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createStoryboard, DURATION } from '../modal-video/motion-graphics/storyboard.mjs';

test('localized demo players, structured data, and real assets describe the same video', async () => {
  const vite = await createServer({ configFile: 'vite.website.config.ts', server: { middlewareMode: true, hmr: false, ws: false }, appType: 'custom' });
  try {
    const { languages, localizedHref } = await vite.ssrLoadModule('/src/localization.ts');
    const { getDemoVideo } = await vite.ssrLoadModule('/src/demo.ts');
    const { SitePage, getSeoMetadata, isIndexablePath } = await vite.ssrLoadModule('/src/pages.tsx');
    const { WebsiteLanguageProvider } = await vite.ssrLoadModule('/src/i18n.tsx');
    const sources = new Set();
    for (const language of languages) {
      const demo = getDemoVideo(language);
      sources.add(demo.src);
      await Promise.all([demo.src, demo.poster, demo.captions].map(file => access(`website/public${file}`)));
      const captions = await readFile(`website/public${demo.captions}`, 'utf8');
      const { narration, frameSvg } = createStoryboard(language);
      assert.equal(narration.at(-1).at + narration.at(-1).duration, DURATION);
      assert.ok(captions.startsWith('WEBVTT\n'));
      for (const cue of narration) assert.ok(captions.includes(cue.text), `${language}: caption matches spoken script`);
      for (const path of ['/', '/demo/']) {
        const html = renderToStaticMarkup(React.createElement(WebsiteLanguageProvider, { language }, React.createElement(SitePage, { pathname: path })));
        assert.ok(html.includes(`src="${demo.src}"`));
        assert.ok(html.includes(`poster="${demo.poster}"`));
        assert.ok(html.includes(`src="${demo.captions}" srcLang="${language}"`));
      }
      const metadata = getSeoMetadata(localizedHref('/demo/', language));
      const video = metadata.schema['@graph'].find(item => item['@type'] === 'VideoObject');
      assert.equal(video.inLanguage, language);
      assert.equal(video.contentUrl, `https://texthalo.app${demo.src}`);
      assert.equal(video.thumbnailUrl, `https://texthalo.app${demo.poster}`);
      assert.equal(video.embedUrl, `https://texthalo.app${localizedHref('/demo/', language)}`);
      assert.equal(video.name, demo.title);
      assert.equal(video.description, demo.description);
      assert.equal(metadata.video.contentUrl, video.contentUrl);
      if (language !== 'en') {
        assert.notEqual(demo.description, getDemoVideo('en').description);
        assert.doesNotMatch(frameSvg(1), /Your words\.|More possibilities\./);
        assert.doesNotMatch(frameSvg(7), /Give your eyes|Select a passage|Speaking/);
      }
      for (const path of ['/sign-in/', '/desktop-connect/', '/reset-password/', '/account/billing/', '/account/billing/success/']) assert.equal(isIndexablePath(localizedHref(path, language)), false);
      assert.equal(isIndexablePath(localizedHref('/demo/', language)), true);
    }
    assert.equal(sources.size, languages.length);
  } finally { await vite.close(); }
});
