import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";
import fs from "node:fs/promises";
import os from "node:os";
import { spawnSync } from "node:child_process";

const require = createRequire(import.meta.url);
const playwrightPath = process.env.PLAYWRIGHT_MODULE || "/tmp/texthalo-motion-render/node_modules/playwright";
const { chromium } = require(playwrightPath);
const here = path.dirname(fileURLToPath(import.meta.url));
const html = path.join(here, "index.html");
const outputDir = path.resolve(here, "../output/marketing");
const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "texthalo-film-"));
const finalPath = path.join(outputDir, "texthalo-product-demo.mp4");
const previewPath = path.join(outputDir, "texthalo-product-demo-preview.png");
const silentPath = path.join(tempDir, "product-demo-silent.mp4");

await fs.mkdir(outputDir, { recursive: true });
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({
  viewport: { width: 1280, height: 720 },
  deviceScaleFactor: 1,
  recordVideo: { dir: tempDir, size: { width: 1280, height: 720 } },
});
const page = await context.newPage();
await page.goto(pathToFileURL(html).href);
await page.evaluate(() => document.fonts.ready);
await page.evaluate(() => window.startFilm());
await page.waitForTimeout(15_350);
const video = page.video();
await context.close();
const webmPath = await video.path();
await browser.close();

const converted = spawnSync("ffmpeg", [
  "-y", "-i", webmPath,
  "-t", "15",
  "-vf", "fps=30,format=yuv420p",
  "-c:v", "libx264", "-crf", "18", "-preset", "medium",
  "-movflags", "+faststart", silentPath,
], { stdio: "inherit" });
if (converted.status !== 0) throw new Error(`ffmpeg exited with ${converted.status}`);

const soundtrack = spawnSync("python3", [path.join(here, "soundtrack.py")], { stdio: "inherit" });
if (soundtrack.status !== 0) throw new Error(`soundtrack generation exited with ${soundtrack.status}`);
const audioPath = path.join(outputDir, "texthalo-original-soundtrack.wav");
const muxed = spawnSync("ffmpeg", [
  "-y", "-i", silentPath, "-i", audioPath,
  "-t", "15", "-map", "0:v:0", "-map", "1:a:0",
  "-c:v", "copy", "-af", "volume=6dB", "-c:a", "aac", "-b:a", "192k",
  "-movflags", "+faststart", finalPath,
], { stdio: "inherit" });
if (muxed.status !== 0) throw new Error(`audio mux exited with ${muxed.status}`);

const preview = spawnSync("ffmpeg", [
  "-y", "-ss", "8.5", "-i", finalPath,
  "-frames:v", "1", "-vf", "scale=960:-1", previewPath, "-loglevel", "error",
], { stdio: "inherit" });
if (preview.status !== 0) throw new Error(`preview export exited with ${preview.status}`);
console.log(`Rendered ${finalPath}`);
