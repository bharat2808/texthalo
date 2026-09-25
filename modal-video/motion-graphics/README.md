# TextHalo product motion graphics

This is a deterministic product overview built from TextHalo's actual website
copy and UI cues, instead of generative video frames. The 15-second storyboard
shows selecting a passage, pressing `⌘⇧S`, listening in the floating player,
and a branded close. It is rendered at 1280x720, 30 fps.

The render script uses Playwright to capture the HTML animation, creates an
original soft synth score and restrained UI sound cues, then uses FFmpeg to
encode H.264 with AAC audio. It writes the MP4, soundtrack WAV, and a still
preview under `modal-video/output/marketing/`.

```sh
npm install --prefix /tmp/texthalo-motion-render playwright
npx --prefix /tmp/texthalo-motion-render playwright install chromium
node modal-video/motion-graphics/render.mjs
```
