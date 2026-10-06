import { expect, test, type Page } from "@playwright/test";

async function boot(page: Page, runtimeInstalled: boolean) {
  await page.setViewportSize({ width: 780, height: 680 });
  await page.addInitScript(({ runtimeInstalled }) => {
    const additional = [
      ["Spanish", ["ef_dora", "em_alex", "em_santa"]],
      ["French", ["ff_siwis"]],
      ["Hindi", ["hf_alpha", "hf_beta", "hm_omega", "hm_psi"]],
      ["Italian", ["if_sara", "im_nicola"]],
      ["Portuguese (Brazil)", ["pf_dora", "pm_alex", "pm_santa"]],
    ] as const;
    const voices = additional.flatMap(([language, ids]) => ids.map(id => ({ id, label: id, language, requires_espeak: true, unavailable: runtimeInstalled ? "Voice files need downloading" : "Needs espeak-ng", note: null })));
    const state = {
      settings: {
        accessibility_prompted: true, onboarding_completed: true, launch_at_login: false, ui_language: "en",
        shortcuts: { speak: "Alt+S", stop: "Alt+X", transcribe: "", assistant: "" },
        assistant: { language: "en-US", instruction: "", dictation_output: "insert", transcription_engine: "apple", provider: "local" },
        engine: "kokoro", voice: null, rate: 200, capture_mode: "copy_only", max_chars: 5000, restore_clipboard: true,
        kokoro: { voice: "af_heart", quant: "fp32", speed: 1, keep_warm: false, idle_unload_minutes: 5 },
        chatterbox: { quant: "q4f16", voice: "en", exaggeration: .1, cfg_weight: .5, ref_audio: null, keep_warm: false },
        fish: { voice_id: "", model_id: "fish-s21-pro", enhance_text: false, privacy_accepted: false },
      }, voices: [], engines: [{ id: "kokoro", label: "Kokoro 82M", summary: "On this Mac", can_speak: true, status: "Ready", needs_download: runtimeInstalled, download_bytes: 6789120,
        voices: [{ id: "af_heart", label: "Heart", language: "American English", requires_espeak: false, unavailable: null, note: null }, ...voices,
          { id: "jf_alpha", label: "Alpha", language: "Japanese", requires_espeak: false, requires_cjk: true, unavailable: "Voice files need downloading", note: null }],
        ref_voices: [], models: [{ id: "fp32", label: "Full precision", description: "Default quality and speed. Largest download.", bytes: 326000000, installed: true }], selected_voice: "af_heart" }],
      trusted: true, focused_password_field: false, speaking: false, refused_shortcuts: [], system_language: "en_US", config_path: "/test/settings.json",
    };
    const calls: string[] = [];
    Object.assign(window, { __state: state, __calls: calls, __native: async (command: string, args: any) => {
      calls.push(command);
      switch (command) {
        case "get_state": return structuredClone(state);
        case "localization_state": return { preference: "en", language: "en" };
        case "save_settings": state.settings = args.settings; return structuredClone(state);
        case "plugin:app|version": return "0.1.15";
        case "get_audio_history": case "generation_history": return [];
        case "desktop_is_signed_in": return false;
        case "desktop_billing_plans": return { plans: [] };
        default: return null;
      }
    } });
  }, { runtimeInstalled });
  await page.route(url => url.pathname === "/src/main.tsx" && !url.searchParams.has("e2e"), route => route.fulfill({ contentType: "application/javascript", body: `
    import { mockIPC, mockWindows } from "/node_modules/@tauri-apps/api/mocks.js";
    mockWindows("main"); mockIPC(window.__native, {shouldMockEvents: true});
    await import("/src/main.tsx?e2e");
  ` }));
  await page.goto("/");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Voices", exact: true }).click();
}

async function emit(page: Page, phase: string, message: string) {
  await page.evaluate(({ phase, message }) => (window as any).__TAURI_INTERNALS__.invoke("plugin:event|emit", {
    event: "kiegen:kokoro-languages", payload: { phase, message },
  }), { phase, message });
}

for (const runtimeInstalled of [false, true]) {
  test(`${runtimeInstalled ? "existing eSpeak installation" : "fresh setup"} uses one action for all additional voice files`, async ({ page }, testInfo) => {
    await boot(page, runtimeInstalled);
    const setup = page.locator(".kokoro-language-setup:not(.kokoro-cjk-setup)");
    await expect(setup).toContainText("Spanish · French · Hindi · Italian · Portuguese (Brazil)");
    if (runtimeInstalled) {
      const advanced = page.locator("details.disclosure").filter({ hasText: "Advanced voice settings" });
      await expect(async () => {
        if (await advanced.getAttribute("open") === null) await advanced.locator("summary").click();
        await expect(advanced).toHaveAttribute("open", "");
      }).toPass();
      const download = page.getByRole("button", { name: "Download missing files", exact: true });
      const remove = page.getByRole("button", { name: "Delete selected model", exact: true });
      const downloadBox = await download.boundingBox();
      const removeBox = await remove.boundingBox();
      expect(downloadBox!.y).toBe(removeBox!.y);
      const setupBox = await setup.boundingBox();
      const buttonBox = await setup.getByRole("button").boundingBox();
      expect(buttonBox!.width).toBeLessThan(setupBox!.width / 2);
      await page.screenshot({ path: testInfo.outputPath("compact-voice-buttons.png") });
    }
    await setup.getByRole("button", { name: "Download 13 voices" }).click();
    await expect(setup.getByRole("button")).toBeDisabled();
    await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((command: string) => command === "install_kokoro_languages").length)).toBe(1);
    expect(await page.evaluate(() => (window as any).__calls.includes("install_espeak_ng"))).toBe(false);
    await emit(page, "voices", "Downloading voice files… Existing model files are reused.");
    await expect(setup.getByRole("button")).toHaveText("Downloading voice files…");
    await page.evaluate(async () => {
      const win = window as any;
      win.__state.engines[0].voices.forEach((voice: any) => { if (voice.requires_espeak) voice.unavailable = null; });
      win.__state.engines[0].needs_download = false;
    });
    await emit(page, "done", "Additional Kokoro voices are ready.");
    await expect(setup).toHaveCount(0);
    await expect(page.getByRole("status")).toContainText("Additional Kokoro voices are ready.");
    await expect(page.locator(".kokoro-cjk-setup")).toBeVisible();
  });
}

test("failed file transfer settles progress and allows retry with eSpeak already installed", async ({ page }) => {
  await boot(page, true);
  const setup = page.locator(".kokoro-language-setup:not(.kokoro-cjk-setup)");
  await setup.getByRole("button", { name: "Download 13 voices" }).click();
  await emit(page, "voices", "Downloading voice files… Existing model files are reused.");
  await page.evaluate(async () => {
    const invoke = (window as any).__TAURI_INTERNALS__.invoke;
    await invoke("plugin:event|emit", { event: "kiegen:install", payload: { engine: "kokoro", quant: "fp32", phase: "downloading", file: "voices/ef_dora.bin", done: 10, total: 100, message: null } });
    await invoke("plugin:event|emit", { event: "kiegen:install", payload: { engine: "kokoro", quant: "fp32", phase: "error", file: "", done: 0, total: 0, message: "Connection interrupted" } });
  });
  await emit(page, "error", "Could not finish voice setup: Connection interrupted. Try again to download only the missing files.");
  await expect(setup.getByRole("button", { name: "Retry voice setup" })).toBeEnabled();
  await setup.getByRole("button", { name: "Retry voice setup" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((command: string) => command === "install_kokoro_languages").length)).toBe(2);
});


test("Japanese and Mandarin files download without requesting eSpeak, and become selectable", async ({ page }) => {
  await boot(page, false);
  await page.evaluate(async () => {
    const win = window as any;
    const ids = ["jf_alpha", "jf_gongitsune", "jf_nezumi", "jf_tebukuro", "jm_kumo", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao", "zf_xiaoyi", "zm_yunjian", "zm_yunxi", "zm_yunxia", "zm_yunyang"];
    const engine = win.__state.engines[0];
    engine.voices = engine.voices.filter((voice: any) => !voice.requires_cjk);
    engine.voices.push(...ids.map(id => ({ id, label: id, language: id.startsWith("j") ? "Japanese" : "Mandarin Chinese", requires_cjk: true, requires_espeak: false, unavailable: "Voice files need downloading", note: null })));
    await win.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "kiegen:install", payload: { engine: "kokoro", quant: "fp32", phase: "done", done: 0, total: 0, file: "", message: null } });
  });
  const setup = page.locator(".kokoro-cjk-setup");
  await expect(setup).toContainText("Pronunciation support is built in.");
  await setup.getByRole("button", { name: "Download 13 voices" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((command: string) => command === "install_engine").length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__calls.includes("install_kokoro_languages"))).toBe(false);
  await page.evaluate(async () => {
    const win = window as any;
    win.__state.engines[0].voices.forEach((voice: any) => { if (voice.requires_cjk) voice.unavailable = null; });
    win.__state.engines[0].needs_download = false;
    await win.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "kiegen:install", payload: { engine: "kokoro", quant: "fp32", phase: "done", done: 0, total: 0, file: "", message: null } });
  });
  await expect(setup).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Preview jf_alpha", exact: true })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Preview zf_xiaobei", exact: true })).toBeEnabled();
  await expect(page.locator(".kokoro-language-setup:not(.kokoro-cjk-setup)").getByRole("button", { name: "Download 13 voices" })).toBeEnabled();
});
