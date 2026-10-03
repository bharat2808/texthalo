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
    import { mockIPC } from "/node_modules/@tauri-apps/api/mocks.js";
    mockIPC(window.__native, { shouldMockEvents: true });
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
