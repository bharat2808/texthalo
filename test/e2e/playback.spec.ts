import { expect, test, type Page } from "@playwright/test";

async function boot(page: Page) {
  await page.setViewportSize({width:350,height:112});
  await page.addInitScript(() => {
    const state = { phase: "speaking" };
    const audio = { position: 20, buffered: 60, duration: null as number | null, paused: false, speed: 1, available: true };
    const commands: any[] = [];
    const books = { books: [], tray: null, generating: null, playback: null };
    Object.assign(window, { __speech: state, __audio: audio, __commands: commands, __books: books, __native: async (command: string, args: any) => {
      commands.push({ method: command, ...args });
      if (command === "localization_state") return { preference: "en", language: "en" };
      if (command === "get_speech_status") return structuredClone(state);
      if (command === "get_playback_status") return structuredClone(audio);
      if (command === "audiobook_state") return structuredClone(books);
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
  await leave(page);
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toHaveCount(0, {timeout:3000});
  await page.locator(".speech-label").hover();
  await page.evaluate(() => { (window as any).__failControl = true; });
  await page.getByRole("button", {name:"Pause", exact:true}).click();
  await expect(page.getByRole("alert")).toHaveText("Playback device unavailable");
});

test("native cursor events reveal controls without a click or DOM mouse-enter", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => (window as any).__TAURI_INTERNALS__.invoke("plugin:event|emit", {
    event: "texthalo:playback-hover", payload: true,
  }));
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__commands.filter((item: any) => item.method === "playback_control").length)).toBe(0);
  await expect.poll(() => page.evaluate(() => document.activeElement?.tagName)).toBe("BODY");
  await page.evaluate(() => (window as any).__TAURI_INTERNALS__.invoke("plugin:event|emit", {
    event: "texthalo:playback-hover", payload: false,
  }));
  await expect(page.getByRole("button", {name:"Pause", exact:true})).toHaveCount(0, {timeout:3000});
});

test("library playback shows live chapter checkpoints without a Kindle suggestion", async ({ page }) => {
  await boot(page);
  await page.setViewportSize({ width: 350, height: 344 });
  await page.evaluate(() => {
    const state = (window as any).__books;
    state.playback = { bookId: "saved-book", audio: (window as any).__audio };
    state.generating = "saved-book";
    state.books = [{ id: "saved-book", title: "Saved Audiobook", activity: "full-scan",
      fullPreparation: { phase: "rendering", transcriptReady: true, completedChapters: 1, chapters: 3 },
      checkpoints: [
        { id: "one", rangeId: "draft", label: "Chapter One", complete: true, seconds: 0, duration: 60 },
        { id: "two", rangeId: "draft", label: "Chapter Two", complete: false, seconds: null, duration: 0 },
      ],
    }];
  });
  await page.locator(".speech-label").hover();
  await expect(page.getByText("Saved Audiobook", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /Chapter One/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Chapter Two/ })).toHaveCount(0);
  await expect(page.getByText("Generating audiobook · 1/3 chapters ready")).toBeVisible();
  await page.evaluate(() => {
    const book = (window as any).__books.books[0];
    Object.assign(book.checkpoints[1], { complete: true, seconds: 60, duration: 75 });
    book.fullPreparation.completedChapters = 2;
  });
  await page.getByRole("button", { name: /Chapter Two/ }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__commands.filter((c: any) => c.method === "audiobook_play"))).toEqual([
    { method: "audiobook_play", id: "saved-book", seconds: 60, rangeId: "draft" },
  ]);
  expect(await page.evaluate(() => (window as any).__books.generating)).toBe("saved-book");
  await expect(page.getByRole("button", { name: "Stop preparing" })).toBeVisible();
  await page.evaluate(() => { (window as any).__books.generating = null; });
  const prepare = page.getByRole("button", { name: "Prepare more chapters" });
  await expect(prepare).toBeEnabled();
  await prepare.click();
  await expect.poll(() => page.evaluate(() => (window as any).__commands.filter((c: any) => c.method === "audiobook_generate"))).toEqual([
    { method: "audiobook_generate", id: "saved-book", full: true },
  ]);
  await page.evaluate(() => { (window as any).__books.books[0].complete = true; });
  await expect(prepare).toHaveCount(0);
  await page.evaluate(() => {
    const book = (window as any).__books.books[0];
    book.complete = false;
    book.fullPreparation.transcriptReady = false;
  });
  await expect(prepare).toHaveCount(0);
});
