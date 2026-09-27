# TextHalo website SEO checklist

## Technical baseline

- Keep every public page statically rendered into its own HTML file at build time. Do not ship an empty app shell for pages that should be indexed.
- Give every indexable route one specific `<title>`, meta description, canonical URL, Open Graph title/description/image, and Twitter card.
- Keep `/robots.txt` and `/sitemap.xml` as real text/XML files. The website build generates both from the route list in `website/src/pages.tsx`.
- Keep a top-level `404.html` page with `noindex`; Cloudflare Pages uses that file for missing routes instead of treating every unknown URL as the single-page app. See [Pages route matching and 404 behavior](https://developers.cloudflare.com/pages/configuration/serving-pages/).
- When adding, removing, or renaming a public route, update the route list and check the generated sitemap. Preserve old URLs with a redirect when a URL must change.
- Use valid, page-specific JSON-LD that matches visible page content. Structured data does not guarantee a search result enhancement.
- Give each important video a dedicated watch page where the video is prominent, visible, and the main reason to visit. Keep its thumbnail and MP4 URL stable; include accurate `VideoObject` metadata and the video sitemap entry.
- Keep the shared navigation links crawlable ordinary `<a href>` links and make sure each article is reachable from its section index and relevant related pages.

## Writing pages and posts

- Write for a real reader first. State what the page answers in its opening paragraph and use one clear `<h1>` per page.
- Use descriptive titles and summaries that accurately describe the page. Avoid repeating the same keyword unnaturally or promising features the app does not have.
- Add useful subheadings, short paragraphs, meaningful internal links, and original images with descriptive alt text where appropriate.
- Show an author/publisher and publish date on editorial pages. Change the date only when the page is materially updated.
- Product-use stories are examples of workflows, not customer case studies. Do not invent customer names, endorsements, performance data, or quotes.
- Recheck factual product statements against the current app and README, especially which speech engines run locally, what uses the network, supported macOS versions, and permissions.

## Before publishing

1. Run `npm run website:build` and inspect the HTML generated under `dist-website/`.
2. Check the homepage, every new article, canonical URLs, JSON-LD, `robots.txt`, and `sitemap.xml` in the output.
3. Deploy the generated directory to the production Cloudflare Pages project and fetch the live routes to confirm they return the intended HTML and metadata.
4. In Google Search Console, verify ownership of `texthalo.app`, submit `https://texthalo.app/sitemap.xml`, and use URL Inspection to request indexing and review crawl/rendering status. Search engines decide whether and when to index pages; a sitemap is a discovery hint, not a guarantee.
5. Review Search Console’s Video indexing report for `/demo/`. A watch page and valid video metadata make the page eligible for video indexing; they do not guarantee that Google will index the video.

## References

- [Google Search Essentials](https://developers.google.com/search/docs/essentials)
- [Google JavaScript SEO basics](https://developers.google.com/search/docs/crawling-indexing/javascript/javascript-seo-basics)
- [Build and submit a sitemap](https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap)
- [Specify a canonical URL](https://developers.google.com/search/docs/crawling-indexing/consolidate-duplicate-urls)
- [Structured data policies](https://developers.google.com/search/docs/appearance/structured-data/sd-policies)
- [Video SEO best practices](https://developers.google.com/search/docs/appearance/video)
- [Video structured data](https://developers.google.com/search/docs/appearance/structured-data/video)
