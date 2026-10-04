# TextHalo website demo

The current demo is a 30-second animated walkthrough of **Listen**, **Dictate**,
and **Assistant**. The scenes use illustrative documents and example results,
with behavior and status labels based on the app source. The on-screen label
identifies it as an animated workflow demo; it is not a live app recording.

`storyboard.mjs` defines the scene layout, motion, narration, and timing. The
renderer draws SVG frames with Sharp, then encodes a 1280×720, 30 fps H.264 MP4
with AAC audio. Fish Audio's official Sophia voice narrates the walkthrough,
with an original quiet synth score and optional English WebVTT captions.

## Render

Use Node.js 22+, FFmpeg (`ffmpeg` and `ffprobe`), and Python 3. Install repository
dependencies with `npm ci`, then run:

```sh
npm run website:demo
npm run website:build
```

Configure `FISH_AUDIO_API_KEY` and optionally `FISH_AUDIO_MODEL` in the environment
or the ignored `texthalo-server/.env`. The default voice is Sophia
(`0efe38923c0b445ab09382e851ae1e30`); `FISH_DEMO_VOICE_ID` can select another Fish
voice. Rendering sends only the public narration script to Fish Audio and uses
that account's generation allowance. API credentials never enter website assets.

Narration takes are cached under the ignored `modal-video/output/marketing/`
directory by text, model, and voice, so visual edits reuse existing speech.
The five takes are fitted to their scenes; the supplied Sophia takes play at
normal speed except for a negligible adjustment to the closing line.

The renderer writes these site assets:

- `website/public/videos/texthalo-workflow-demo.mp4`
- `website/public/videos/texthalo-workflow-demo-poster.png`
- `website/public/videos/texthalo-workflow-demo.vtt`

It also prints the location of a five-scene contact sheet for review. Website
players and metadata are configured in `website/src/demo.ts`.
