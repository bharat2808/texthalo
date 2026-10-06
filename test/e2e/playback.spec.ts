import { expect, test, type Page } from "@playwright/test";

async function boot(page: Page) {
  await page.setViewportSize({width:350,height:112});
  await page.addInitScript(() => {
    const state = { phase: "speaking" };
    const audio = { position: 20, buffered: 60, duration: null as number | null, paused: false, speed: 1, available: true };
    const commands: any[] = [];
    Object.assign(window, { __speech: state, __audio: audio, __commands: commands, __native: async (command: string, args: any) => {
      commands.push({ method: command, ...args });
      if (command === "localization_state") return { preference: "en", language: "en" };
      if (command === "get_speech_status") return structuredClone(state);
      if (command === "get_playback_status") return structuredClone(audio);
      if (command === "assistant_state") return { status: { phase: "idle" } };
      if (command === "stop_speaking") { state.phase = "idle"; audio.available = false; }
      if (command === "playback_control") {
        if ((window as any).__failControl) throw new Error("Playback device unavailable");
        if (args.command === "pause") audio.paused = true;
        if (args.command === "resume") audio.paused = false;
        if (args.command === "speed") audio.speed = args.value;
        if (args.command === "seek") audio.position = Math.max(0, Math.min(args.value, audio.buffered));
        if (args.command === "replay") { audio.position = 0; audio.paused = false; state.phase = "speaking"; }
      }
      return null;
    } });
  });
  await page.route(url => url.pathname === "/src/main.tsx" && !url.searchParams.has("e2e"), route => route.fulfill({ contentType: "application/javascript", body: `
    import { mockIPC, mockWindows } from "/node_modules/@tauri-apps/api/mocks.js";
    mockWindows("speech-overlay");
    mockIPC(window.__native, {shouldMockEvents: true});
    await import("/src/main.tsx?e2e");
  ` }));
  await page.goto("/?overlay");
  await expect(page.getByRole("status")).toHaveText("Speaking", {timeout:15000});
}

async function leave(page: Page) {
  await page.locator(".speech-label").evaluate(el => (el as HTMLElement).focus());
  await page.evaluate(() => { (document.activeElement as HTMLElement)?.blur(); });
  await page.mouse.move(700, 400);
}

test("hover reveals real commands; controls collapse after leaving", async ({ page }) => {
  await boot(page);
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toHaveCount(0);
  await page.locator(".speech-label").hover();
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toBeVisible();
  await page.getByRole("button", {name:"Back 10 seconds"}).click();
  await expect(page.getByRole("slider")).toHaveValue("10");
  await page.getByRole("button", {name:"Forward 10 seconds"}).click();
  await expect(page.getByRole("slider")).toHaveValue("20");
  await page.screenshot({path:"/tmp/texthalo-playback-controls.png",omitBackground:true});
  await page.getByRole("combobox", {name:"Playback speed"}).selectOption("1.5");
  await expect(page.getByRole("combobox")).toHaveValue("1.5");
  await page.getByRole("slider").fill("40");
  await expect.poll(() => page.evaluate(() => (window as any).__audio.position)).toBe(40);
  await leave(page);
  await page.waitForTimeout(800);
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toBeVisible();
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toHaveCount(0, {timeout:3000});
  const commands = await page.evaluate(() => (window as any).__commands);
  expect(commands).toContainEqual({method:"playback_control", command:"speed", value:1.5});
});

test("paused playback retains Resume; Stop removes the widget", async ({ page }) => {
  await boot(page);
  await page.locator(".speech-label").hover();
  await page.getByRole("button", {name:"Pause", exact:true}).click();
  await expect(page.getByRole("status")).toHaveText("Paused");
  await leave(page);
  await page.waitForTimeout(1800);
  await expect(page.getByRole("button", {name:"Resume", exact:true})).toBeVisible();
  await page.getByRole("button", {name:"Resume", exact:true}).click();
  await expect(page.getByRole("status")).toHaveText("Speaking", {timeout:15000});
  await page.getByRole("button", {name:"Stop speech", exact:true}).click();
  await expect(page.locator(".playback-overlay")).toHaveCount(0);
});

test("completed reading exposes Replay and control failures stay visible", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => { (window as any).__speech.phase = "idle"; (window as any).__audio.duration = 60; });
  await expect(page.getByRole("status")).toHaveText("Finished");
  await page.getByRole("button", {name:"Replay", exact:true}).click();
  await expect(page.getByRole("status")).toHaveText("Speaking", {timeout:15000});
  await page.evaluate(() => { (window as any).__failControl = true; });
  await page.getByRole("button", {name:"Pause", exact:true}).click();
  await expect(page.getByRole("alert")).toHaveText("Playback device unavailable");
});
