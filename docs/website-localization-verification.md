# Website localization

Implemented on `codex/desktop-localization`, alongside the existing desktop localization.

The website shares the desktop language resolver, interpolation helpers, and existing translations. Website-specific Canadian French and Latin American Spanish catalogs cover marketing, pricing, account and authentication screens, privacy policy, blog articles, stories, accessibility labels, and metadata.

Unprefixed URLs use the saved website preference or the browser’s preferred supported language, with English fallback. Explicit language URLs take precedence. The header picker persists a manual choice in local storage. Explicit `/en/` URLs also preserve English when storage is disabled. The desktop continues to use the Mac’s preferred interface language; download region does not select the app language. Website and desktop preferences remain independent.

The build prerenders 16 routes in English, French, and Spanish, plus explicit English aliases. Translated HTML is available without JavaScript. Canonicals, alternate-language links, sitemap entries, page descriptions, social metadata, structured data, and localized 404 pages are generated. English aliases canonicalize to the existing English URLs. Client hydration uses the URL’s language, then automatic selection runs after hydration.

The billing API remains unchanged. Prices, currencies, credits, and clone limits come from its existing response. Known descriptions and feature messages translate at presentation; numeric feature values use `creditsPerPeriod` and `cloneLimit`, including the backend’s effective clone cap. Unknown backend text retains its source. Existing offline pricing remains available and is translated. Internal navigation, sign-in, reset-password, and desktop-connect links preserve language and query parameters. Emails, passwords, tokens, identifiers, and redirect validation are not translated.

The demo’s audio and caption track remain English. Stripe checkout and Neon Auth emails are external services and are not translated by the website catalogs. Unknown provider error messages retain their source text.

## Verification

- `npm test`: 66 passed, zero failed. Covers catalog parameter parity, all editorial translations, all localized route metadata and SSR output, real backend-value formatting, unknown-text fallback, and existing app contracts.
- `npm run website:build`: passed TypeScript, Vite, and prerendering of all 64 route variants and localized 404s. Vite retains its large-chunk warning.
- `npm run build`: passed the desktop TypeScript and Vite build.
- `npm run website:test:e2e`: nine Chromium tests passed against the production build. Covers browser detection, persistence, explicit language precedence, preserved connect parameters, live API values, unsupported-language fallback, JavaScript-disabled article/SEO rendering, blocked storage, and desktop/mobile header layout.
- French pricing reviewed visually at desktop width; long navigation labels and the language picker fit within the header.
- `git diff --check`: passed.

No website deployment or new desktop release was performed for this change.
