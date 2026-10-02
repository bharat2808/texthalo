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
test("read-only target offers copy and speech but cannot insert", async ({ page }) => {
  await boot(page);
  await expect(page.getByLabel("Generated text")).toHaveText("A concise summary.");
  await expect(page.getByRole("button", { name: "Insert / Replace" })).toBeDisabled();
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  await page.getByRole("button", { name: "Speak", exact: true }).click();
  const calls = await page.evaluate(() => (window as any).__calls.filter((c: any) => c.command === "assistant_control").map((c: any) => c.args.command));
  expect(calls).toEqual(["copy", "speak"]);
});
test("editable target insertion is explicit and follow-up sends typed request", async ({ page }) => {
  await boot(page, "result", true);
  await page.getByRole("button", { name: "Insert / Replace" }).click();
  await page.getByLabel("Request or follow-up").fill("Make it shorter");
  await page.getByRole("button", { name: "Ask assistant" }).click();
  await expect(page.getByLabel("Request or follow-up")).toHaveValue("");
  const calls = await page.evaluate(() => (window as any).__calls.filter((c: any) => c.command === "assistant_control"));
  expect(calls.map((c: any) => c.args)).toEqual([{command:"insert"}, {command:"ask",text:"Make it shorter"}]);
});
test("recording offers cancellation without result or screenshot actions", async ({ page }) => {
  await boot(page, "recording");
  await expect(page.getByRole("status")).toContainText("Release to transcribe");
  await expect(page.getByRole("button", { name: "Copy", exact: true })).toHaveCount(0);
  await expect(page.getByText("Add screen text", { exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect.poll(async () => page.evaluate(() => (window as any).__calls.some((c: any) => c.args?.command === "cancel"))).toBeTruthy();
});
test("screenshot context requires an explicit choice and is labeled OCR", async ({ page }) => {
  await boot(page);
  await page.getByText("Add screen text", { exact: true }).click();
  await expect(page.getByText(/Local OCR extracts visible text/)).toBeVisible();
  await page.getByRole("button", { name: "Choose region" }).click();
  const calls = await page.evaluate(() => (window as any).__calls.filter((c: any) => c.command === "assistant_control"));
  expect(calls.map((c: any) => c.args)).toEqual([{command:"screen",context:"region"}]);
});
test("speech destination notice refreshes when the engine changes", async ({ page }) => {
  await boot(page);
  await expect(page.getByText(/your selected voice run on this Mac/)).toBeVisible();
  await page.evaluate(async () => {
    // Exercise the production event listener through Tauri's event mock.
    const events = await import("/node_modules/@tauri-apps/api/event.js");
    await events.emit("texthalo:assistant", { id:8, phase:"result", text:"New result", hostedSpeech:true });
  });
  await expect(page.getByText(/Speaking sends text to the hosted Fish voice service/)).toBeVisible();
});
