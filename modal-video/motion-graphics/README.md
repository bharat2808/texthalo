# TextHalo website demo

The current demo is a 30-second animated walkthrough of **Listen**, **Dictate**,
and **Assistant**. The scenes use illustrative documents and example results,
with behavior and status labels based on the app source. The on-screen label
identifies it as an animated workflow demo; it is not a live app recording.

`storyboard.mjs` defines the shared scene layout, motion, and timing.
`locales.json` supplies narration and on-screen text for all eight website languages. The
renderer draws SVG frames with Sharp, then encodes a 1280×720, 30 fps H.264 MP4
with AAC audio. Fish Audio's official Sophia voice narrates the walkthrough,
with an original quiet synth score and optional localized WebVTT captions.
The same voice ID and animation template are reused for every language. Text
is measured with Sharp before rendering and sized to fit the scene layout.

## Render

Use Node.js 22+, FFmpeg (`ffmpeg` and `ffprobe`), and Python 3. Install repository
dependencies with `npm ci`, then run:

```sh
npm run website:demo -- --all
npm run website:build
```

Configure `FISH_AUDIO_API_KEY` and optionally `FISH_AUDIO_MODEL` in the environment
or the ignored `texthalo-server/.env`. The default voice is Sophia
(`0efe38923c0b445ab09382e851ae1e30`); `FISH_DEMO_VOICE_ID` can select another Fish
voice. Rendering sends only the public narration script to Fish Audio and uses
that account's generation allowance. API credentials never enter website assets.

Narration takes are cached under the ignored `modal-video/output/marketing/`
directory by text, model, and voice, so visual edits reuse existing speech.
The five takes are fitted to their scenes. A take that needs more than 1.35×
playback speed is rejected so its translation can be shortened. English takes
are reused from the existing cache. Changing a translation regenerates only
that take. Use `npm run website:demo -- --language fr-CA` to render one locale;
without flags, the command renders English.

The renderer writes these English site assets:

- `website/public/videos/texthalo-workflow-demo.mp4`
- `website/public/videos/texthalo-workflow-demo-poster.png`
- `website/public/videos/texthalo-workflow-demo.vtt`

Other languages add their locale to the basename, for example
`texthalo-workflow-demo-fr-CA.mp4`, with matching poster and captions. Keep the
publication date in `website/src/demo.ts` accurate when replacing published
assets. Run `npm run website:build` after **all** requested renders complete so
the deployment includes every localized asset.

It also prints the location of a five-scene contact sheet for review. Website
players and metadata are configured in `website/src/demo.ts`.

Both the homepage and watch page select assets using the website language.
The prerendered Open Graph tags, VideoObject (including `inLanguage`), article
images, and video sitemap use the same localized URLs. Account/authentication
pages are marked `noindex` and omitted from the sitemap. After rendering and
building, run `node --test scripts/website-demo.test.mjs` and
`npm run website:test:e2e -- test/e2e-website/demo.spec.ts` to check media,
localization, and metadata consistency.
