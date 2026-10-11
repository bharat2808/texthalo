import { expect, test } from "@playwright/test";

test("finished chapters appear and can play while full-book generation continues", async ({ page }) => {
  await page.addInitScript(() => {
    const book = {
      id: "book", key: "kindle:book", title: "Growing Book", platform: "kindle",
      pages: 0, generated: 0, totalPages: 20, totalGenerated: 0, totalSeconds: 0,
      generatedSeconds: 0, seconds: 0, position: 0, speed: 1, complete: false, fromBeginning: false,
      message: null, activity: "full-scan", resume: null, pendingPage: null,
      checkpoints: [], passage: null,
      fullPreparation: { chapters: 3, completedChapters: 0, transcriptReady: true,
        total: 30, phase: "rendering", pages: 20, page: "Page 20", generated: 0, reused: 0 },
    };
    const state = { books: [book], tray: null, readerConnected: false, generating: "book", playback: null };
    const calls: unknown[] = [];
    Object.assign(window, { __books: state, __calls: calls, __native: async (command: string, args: any) => {
      calls.push({ command, args });
      if (command === "audiobook_state") return structuredClone(state);
      return null;
    } });
  });
  await page.route("**/src/main.tsx", async route => {
    const response = await route.fetch();
    const body = await response.text();
    await route.fulfill({ contentType: "application/javascript", body: `
      import { mockIPC } from "/node_modules/@tauri-apps/api/mocks.js";
      mockIPC(window.__native, { shouldMockEvents: true });
      ${body.replace('/src/App.tsx', '/src/Audiobooks.tsx')}
    ` });
  });
  await page.goto("/");
  await expect(page.getByText("TextHalo Reader disconnected", { exact: true })).toBeVisible();
  await expect(page.getByText(/click TextHalo Reader, then choose Connect/)).toBeVisible();
  // Connection status is independent of a readable book or a dismissed reader tray.
  await page.evaluate(() => { (window as any).__books.readerConnected = true; });
  await expect(page.getByText("TextHalo Reader connected", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /Get TextHalo Reader/ })).toHaveCount(0);
  await page.getByRole("button", { name: /Growing Book/ }).click();
  await expect(page.getByText("No completed chapters yet.")).toBeVisible();
  await page.evaluate(() => {
    const book = (window as any).__books.books[0];
    book.fullPreparation.completedChapters = 1;
    book.totalSeconds = 120;
    book.generatedSeconds = 160;
    book.totalGenerated = 10;
    book.resume = { chapter: "Chapter One", page: "Page 1", seconds: 30, rangeId: "draft" };
    book.checkpoints = [
      { id: "one", rangeId: "draft", status: "complete", label: "Chapter One",
        complete: true, duration: 120, seconds: 0, sentence: 0, page: "Page 1", needsEarlier: false },
      { id: "two", rangeId: "draft", status: "preparing", label: "Chapter Two",
        complete: false, duration: 0, seconds: null, sentence: 10, page: "Page 8", needsEarlier: false },
    ];
  });
  await expect(page.getByText("Generating audiobook · 1/3 chapters ready")).toBeVisible();
  await expect(page.getByText("2:00 ready", { exact: true })).toBeVisible();
  await expect(page.getByText("2:00 playable", { exact: true })).toBeVisible();
  await expect(page.getByRole("slider", { name: "Audiobook playback position" })).toHaveAttribute("max", "120");
  await expect(page.getByRole("slider", { name: "Audiobook playback position" })).toHaveValue("30");
  await expect(page.getByRole("button", { name: /Chapter Two/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Continue listening" })).toBeEnabled();
  await page.getByRole("button", { name: /Chapter One Complete/ }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((c: any) => c.command === "audiobook_play"))).toEqual([
    { command: "audiobook_play", args: { id: "book", seconds: 0, rangeId: "draft" } },
  ]);
  expect(await page.evaluate(() => (window as any).__books.generating)).toBe("book");
  expect(await page.evaluate(() => (window as any).__calls.some((c: any) => c.command === "audiobook_stop_generation"))).toBe(false);
  await page.evaluate(() => {
    (window as any).__books.generating = null;
    (window as any).__books.readerConnected = false;
  });
  await expect(page.getByText("TextHalo Reader disconnected", { exact: true })).toBeVisible();
  const prepare = page.getByRole("button", { name: "Prepare more chapters" });
  await expect(prepare).toBeEnabled();
  await prepare.click();
  await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((c: any) => c.command === "audiobook_generate"))).toEqual([
    { command: "audiobook_generate", args: { id: "book", full: true } },
  ]);
});
