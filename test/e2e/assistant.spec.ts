import { expect, test, type Page } from "@playwright/test";

async function boot(page: Page, phase = "result", canInsert = false) {
  await page.addInitScript(({ phase, canInsert }) => {
    const status = { id: 7, phase, message: phase === "recording" ? "Listening… Release to transcribe." : "Ready", text: phase === "result" ? "A concise summary." : "", canInsert, source: "TextEdit", context: "Original passage" };
    Object.assign(window, { __calls: [], __native: async (command: string, args: any) => {
      (window as any).__calls.push({ command, args });
      if (command === "assistant_state") return { status, availability: {} };
      if (command === "get_state") return { settings: { engine: "apple" } };
      if (command === "assistant_control") return null;
      throw new Error(command);
    } });
  }, { phase, canInsert });
  await page.route("**/src/main.tsx", route => route.fulfill({ contentType: "application/javascript", body: `
    import { mockIPC, mockWindows } from "/node_modules/@tauri-apps/api/mocks.js";
    mockIPC(window.__native, { shouldMockEvents: true });
    mockWindows("assistant");
    await import("/src/main.tsx?e2e");
  ` }));
  await page.goto("/?assistant");
}
test("applied edit clears the overlay without showing a success panel", async ({ page }) => {
  await boot(page);
  await expect(page.getByLabel("Generated text")).toHaveText("A concise summary.");
  await page.evaluate(async () => {
    const events = await import("/node_modules/@tauri-apps/api/event.js");
    await events.emit("texthalo:assistant", { id:7, phase:"result", action:"applied", message:"Text replaced", canUndo:true, text:"A concise summary." });
  });
  await expect(page.locator("#root")).toBeEmpty();
});
test("unavailable target retains a copyable result without insertion", async ({ page }) => {
  await boot(page);
  await expect(page.getByLabel("Generated text")).toHaveText("A concise summary.");
  await expect(page.getByRole("button", { name: "Insert / Replace" })).toHaveCount(0);
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  expect(await page.evaluate(() => (window as any).__calls.at(-1).args)).toEqual({command:"copy",requestId:7});
});
test("recording preserves cancellation without a composer", async ({ page }) => {
  await boot(page, "recording");
  await expect(page.getByRole("status")).toContainText("Listening");
  await expect(page.getByRole("textbox")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect.poll(async () => page.evaluate(() => (window as any).__calls.some((c: any) => c.args?.command === "cancel"))).toBeTruthy();
});
test("cloud response shows generation usage without adding a composer", async ({ page }) => {
  await boot(page);
  await expect(page.getByLabel("Generated text")).toBeVisible();
  await page.evaluate(async () => {
    const { emit } = await import("/node_modules/@tauri-apps/api/event.js");
    await emit("texthalo:assistant", { id: 7, phase: "result", provider: "cloud", text: "A cloud answer.", message: "Ready", usage: { billingStatus: "settled", chargedCredits: 12, refundedCredits: 88 } });
  });
  await expect(page.getByText("TextHalo Cloud · 12 credits for generation")).toBeVisible();
  await expect(page.getByRole("textbox")).toHaveCount(0);
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  expect(await page.evaluate(() => (window as any).__calls.at(-1).args)).toEqual({ command: "copy", requestId: 7 });
});

test("dictation and generation share the compact activity overlay", async ({page}, testInfo) => {
  await page.emulateMedia({colorScheme:"dark"});
  await page.setViewportSize({width:270,height:44});
  await boot(page, "recording");
  await expect(page.locator(".speech-overlay")).toBeVisible();
  await expect(page.locator(".speech-overlay")).toHaveCSS("background-color", "rgb(50, 50, 50)");
  await expect(page.locator(".activity i").first()).toHaveCSS("background-color", "rgb(10, 132, 255)");
  await expect(page.getByLabel("Generated text")).toHaveCount(0);
  await page.screenshot({path:testInfo.outputPath("listening.png")});
  await page.evaluate(async () => {
    const {emit} = await import("/node_modules/@tauri-apps/api/event.js");
    await emit("texthalo:assistant", {id:7,phase:"processing",message:"Thinking on this Mac…",mode:"assistant"});
  });
  await expect(page.locator(".speech-overlay")).toBeVisible();
  await expect(page.getByRole("status")).toHaveText("Generating…");
  await page.screenshot({path:testInfo.outputPath("generating.png")});
  await page.getByRole("button", {name:"Cancel"}).click();
  expect(await page.evaluate(() => (window as any).__calls.at(-1).args.command)).toBe("cancel");
});

test("fallback results use the TextHalo dark palette", async ({page}) => {
  await page.emulateMedia({colorScheme:"dark"});
  await boot(page);
  await expect(page.locator(".assistant-panel")).toHaveCSS("background-color", "rgb(50, 50, 50)");
  await expect(page.locator(".assistant-result")).toHaveCSS("background-color", "rgb(30, 30, 30)");
});

test("fallback header drags the window without stealing button interactions", async ({page}) => {
  await boot(page);
  await page.locator(".assistant-panel header").hover();
  await page.mouse.down();
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => (window as any).__calls)).toContainEqual(expect.objectContaining({command:"plugin:window|start_dragging"}));
  await page.getByRole("button", {name:"Dismiss"}).dispatchEvent("mousedown", {button:0});
  expect(await page.evaluate(() => (window as any).__calls.filter((call: any) => call.command === "plugin:window|start_dragging").length)).toBe(1);
});

for (const phase of ["recording", "streaming", "speaking", "processing"]) {
  test(`${phase} shows a capped live preview and request-owned cancellation`, async ({page}) => {
    await page.setViewportSize({width:270,height:140});
    await boot(page, "recording");
    await expect(page.getByRole("status")).toContainText("Listening");
    await page.evaluate(async ({phase}) => {
      const {emit} = await import("/node_modules/@tauri-apps/api/event.js");
      await emit("texthalo:assistant", {id:7, phase, action:"preview", text:"Live answer. ".repeat(500), message:"Working…"});
    }, {phase});
    const preview = page.getByLabel(phase === "recording" ? "Live transcript" : "Generated text");
    await expect(preview).toBeVisible();
    await expect(page.locator(".speech-overlay .speech-status")).toBeVisible();
    await expect(page.locator(".assistant-panel")).toHaveCount(0);
    const bar = await page.locator(".speech-status").boundingBox();
    const textBox = await preview.boundingBox();
    expect(textBox!.y).toBeGreaterThanOrEqual(bar!.y + bar!.height);
    expect((await preview.textContent())!.length).toBeLessThanOrEqual(2001);
    expect(await preview.evaluate(element => element.scrollTop + element.clientHeight >= element.scrollHeight - 1)).toBeTruthy();
    await expect(page.getByRole("button", {name:"Copy", exact:true})).toHaveCount(0);
    await page.getByRole("button", {name:"Cancel", exact:true}).click();
    expect(await page.evaluate(() => (window as any).__calls.at(-1).args)).toEqual({command:"cancel",requestId:7});
  });
}

test("streamed speech keeps one preview through playback and cancellation", async ({page}) => {
  await boot(page, "recording");
  await expect(page.getByRole("status")).toContainText("Listening");
  for (const phase of ["streaming", "speaking"]) {
    await page.evaluate(async ({phase}) => {
      const {emit} = await import("/node_modules/@tauri-apps/api/event.js");
      await emit("texthalo:assistant", {id:7, phase, action:"speak", text:"A spoken answer.", message:phase === "speaking" ? "Speaking…" : "Speaking · generating…"});
    }, {phase});
    await expect(page.getByLabel("Generated text")).toHaveText("A spoken answer.");
    await expect(page.getByRole("status")).toContainText("Speaking");
    await expect(page.getByRole("button", {name:"Copy", exact:true})).toHaveCount(0);
    await expect(page.getByRole("button", {name:"Cancel", exact:true})).toBeVisible();
  }
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => (window as any).__calls.at(-1).args)).toEqual({command:"cancel", requestId:7});
});

for (const phase of ["error", "result"]) {
  test(`${phase} retains a partial answer with Copy and scoped Undo`, async ({page}) => {
    await boot(page, "recording");
    await expect(page.getByRole("status")).toContainText("Listening");
    await page.evaluate(async ({phase}) => {
      const {emit} = await import("/node_modules/@tauri-apps/api/event.js");
      await emit("texthalo:assistant", {id:7, phase, action:"preview", text:"A partial answer.", canUndo:true, message:"Generation stopped."});
    }, {phase});
    await expect(page.getByLabel("Generated text")).toHaveText("A partial answer.");
    await page.getByRole("button", {name:"Copy", exact:true}).click();
    expect(await page.evaluate(() => (window as any).__calls.at(-1).args)).toEqual({command:"copy",requestId:7});
    await page.getByRole("button", {name:"Undo", exact:true}).click();
    expect(await page.evaluate(() => (window as any).__calls.at(-1).args)).toEqual({command:"undo",requestId:7});
  });
}
