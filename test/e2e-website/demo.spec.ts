import { test, expect } from '@playwright/test';

for (const language of ['en', 'fr-CA', 'es-419', 'pt-BR', 'it', 'ja', 'zh-Hans', 'zh-Hant']) {
  test(`${language}: statically rendered demo media and SEO agree with the video sitemap`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled: false });
    const page = await context.newPage();
    const prefix = language === 'en' ? '' : `/${language}`;
    const basename = `/videos/texthalo-workflow-demo${language === 'en' ? '' : `-${language}`}`;
    await page.goto(`${prefix}/demo/`);
    const video = page.locator('video');
    await expect(video.locator('source')).toHaveAttribute('src', `${basename}.mp4`);
    await expect(video.locator('track')).toHaveAttribute('src', `${basename}.vtt`);
    await expect(video.locator('track')).toHaveAttribute('srclang', language);
    await expect(video).toHaveAttribute('poster', `${basename}-poster.png`);
    await expect(page.locator('meta[property="og:video"]')).toHaveAttribute('content', `https://texthalo.app${basename}.mp4`);
    await expect(page.locator('meta[property="og:image"]')).toHaveAttribute('content', `https://texthalo.app${basename}-poster.png`);
    const html = await page.content();
    const schema = JSON.parse(html.match(/<script type="application\/ld\+json">([^<]+)<\/script>/)![1]);
    const structuredVideo = schema['@graph'].find((item: Record<string, unknown>) => item['@type'] === 'VideoObject');
    expect(structuredVideo.inLanguage).toBe(language);
    expect(structuredVideo.contentUrl).toBe(`https://texthalo.app${basename}.mp4`);
    for (const extension of ['.mp4', '.vtt', '-poster.png']) {
      const response = await page.request.get(`${basename}${extension}`);
      expect(response.ok()).toBe(true);
      expect(response.headers()['content-type']).toContain(extension === '.mp4' ? 'video/mp4' : extension === '.vtt' ? 'text/vtt' : 'image/png');
    }
    const sitemap = await (await page.request.get('/sitemap.xml')).text();
    const entry = sitemap.split('<url>').find(entry => entry.startsWith(`<loc>https://texthalo.app${prefix}/demo/</loc>`))!;
    expect(entry).toContain(`<video:content_loc>${structuredVideo.contentUrl}</video:content_loc>`);
    expect(entry).toContain(`<video:thumbnail_loc>${structuredVideo.thumbnailUrl}</video:thumbnail_loc>`);
    expect(entry).toContain('hreflang="x-default"');
    expect(sitemap).not.toMatch(/<loc>[^<]*(?:sign-in|desktop-connect|reset-password|account\/billing)/);
    await context.close();
  });
}
