import { expect, test, type Page } from "@playwright/test";

async function boot(page: Page, language = "fr-CA", path = "/", onboarding = false) {
  await page.addInitScript(({ language, onboarding }) => {
    const saved = JSON.parse(localStorage.getItem("localization-settings") || "null");
    const preference = saved?.ui_language ?? "system";
    const effective = preference === "system" ? language : preference;
    const state = { settings: saved ?? {
      accessibility_prompted: true, onboarding_completed: !onboarding, launch_at_login: false, ui_language: preference,
      shortcuts: { speak: "Alt+S", stop: "Alt+X", transcribe: "", assistant: "" },
      assistant: { language: "en-US", instruction: "My English instructions", dictation_output: "insert", transcription_engine: "apple", provider: "local" },
      engine: "apple", voice: null, rate: 200, capture_mode: "copy_only", max_chars: 5000, restore_clipboard: true,
      kokoro: { voice: "af_heart", quant: "fp32", speed: 1, keep_warm: false, idle_unload_minutes: 5 },
      chatterbox: { quant: "q4f16", voice: "en", exaggeration: .1, cfg_weight: .5, ref_audio: null, keep_warm: false },
      fish: { voice_id: "", model_id: "fish-s21-pro", enhance_text: false, privacy_accepted: false },
    }, voices: [], engines: [{ id: "apple", label: "Apple system voices", can_speak: true, status: "Ready", needs_download: false, download_bytes: 0, voices: [], ref_voices: [], models: [], selected_voice: "" }],
      trusted: true, focused_password_field: false, speaking: false, refused_shortcuts: [], system_language: "en_US", config_path: "/test/settings.json" };
    if ((window as any).__referenceVoice) {
      state.settings.engine = "chatterbox";
      state.engines = [{ ...state.engines[0], id: "chatterbox", label: "Chatterbox", ref_voices: [
        { id: "builtin", label: "Built-in voice", note: "", builtin: true },
        { id: "/test/Ready.wav", label: "Ready", note: "", builtin: false },
      ] }] as any;
    }
    Object.assign(window, { __state: state, __native: async (command: string, args: any) => {
      switch (command) {
        case "get_state": return structuredClone(state);
        case "localization_state": return { preference: state.settings.ui_language, language: effective };
        case "save_settings": {
          if ((window as any).__failSave) throw new Error("Disk unavailable");
          state.settings = args.settings;
          localStorage.setItem("localization-settings", JSON.stringify(args.settings));
          await (window as any).__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "texthalo:language", payload: { preference: args.settings.ui_language, language: args.settings.ui_language === "system" ? language : args.settings.ui_language } });
          return structuredClone(state);
        }
        case "plugin:app|version": return "0.1.14";
        case "get_audio_history": return [];
        case "generation_history": {
          const run = { id: "test-run", createdAt: 1700000000000, request: "My English request", source: "Test editor", status: "result",
            events: [{ phase: "streaming", at: 1700000000000, message: "My diagnostic" }] };
          return args?.id ? run : [run];
        }
        case "desktop_is_signed_in": return false;
        case "desktop_billing_plans": return { plans: [] };
        case "assistant_state": return { availability: { model: "available", microphone: 3, recognition: 3, accessibility: true }, status: { id: 1, phase: "recording", mode: "assistant", text: "My English words", message: "Listening… Release to transcribe." } };
        case "get_update_overlay_status": return {visible:true,version:null,installable:false,installing:false,message:"Checking for updates…",error:false};
        case "get_speech_status": return { phase: "preparing" };
        default: return null;
      }
    }});
  }, { language, onboarding });
  await page.route(url => url.pathname === "/src/main.tsx" && !url.searchParams.has("e2e"), route => route.fulfill({ contentType: "application/javascript", body: `
    import { mockIPC, mockWindows } from "/node_modules/@tauri-apps/api/mocks.js";
    mockWindows("main");
    mockIPC(window.__native, {shouldMockEvents: true});
    await import("/src/main.tsx?e2e");
  ` }));
  await page.goto(path);
  // Vite may still be transforming the initial module graph after navigation.
  await expect(page.locator("#root")).not.toBeEmpty({ timeout: 15000 });
}

test("Mac language initializes the UI independently of speech language and manual choice persists", async ({ page }) => {
  await boot(page);
  await expect(page.getByRole("heading", { name: "Accueil", exact: true })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("lang", "fr-CA");
  await page.getByRole("button", { name: "Paramètres", exact: true }).click();
  await page.getByLabel("Langue de l’interface").selectOption("es-419");
  await expect(page.getByRole("heading", { name: "General", exact: true })).toBeVisible();
  await expect(page.getByLabel("Idioma de la interfaz")).toHaveValue("es-419");
  await expect(page.locator("html")).toHaveAttribute("lang", "es-419");
  expect(await page.evaluate(() => (window as any).__state.settings.assistant.language)).toBe("en-US");
  expect(await page.evaluate(() => (window as any).__state.settings.assistant.instruction)).toBe("My English instructions");
  await page.reload();
  await expect(page.getByRole("heading", { name: "Inicio", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Configuración", exact: true }).click();
  await page.getByLabel("Idioma de la interfaz").selectOption("system");
  await expect(page.getByRole("heading", { name: "Général", exact: true })).toBeVisible();
});
test("setup starts in the selected Mac language", async ({ page }) => {
  await boot(page, "es-419", "/", true);
  await expect(page.getByText("Bienvenido a TextHalo", {exact: true})).toBeVisible();
  await expect(page.getByRole("button", { name: "Omitir configuración" })).toBeVisible();
});
test("speech window uses persisted language and updates while open", async ({ page }) => {
  await boot(page, "fr-CA", "/?overlay");
  await expect(page.getByRole("status")).toHaveText("Préparation de la voix…");
  await page.evaluate(() => (window as any).__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "texthalo:language", payload: { preference: "es-419", language: "es-419" } }));
  await expect(page.getByRole("status")).toHaveText("Preparando voz…");
});
test("Assistant labels translate and user transcript remains unchanged", async ({ page }) => {
  await boot(page, "es-419", "/?assistant");
  await expect(page.getByRole("status")).toContainText("Escuchando…");
  await expect(page.getByLabel("Transcripción en vivo")).toContainText("My English words");
});

test("update status changes language while open without changing its state", async ({page}) => {
  await boot(page, "fr-CA", "/?update-overlay");
  await expect(page.getByRole("heading")).toHaveText("Recherche de mises à jour…");
  await page.evaluate(() => (window as any).__TAURI_INTERNALS__.invoke("plugin:event|emit", {event:"texthalo:language",payload:{preference:"es-419",language:"es-419"}}));
  await expect(page.getByRole("heading")).toHaveText("Buscando actualizaciones…");
});


test("custom reference voice names remain unchanged even when they match an interface message", async ({ page }) => {
  await page.addInitScript(() => { (window as any).__referenceVoice = true; });
  await boot(page, "fr-CA");
  await page.getByRole("button", { name: "Paramètres", exact: true }).click();
  await page.getByRole("button", { name: "Voix", exact: true }).click();
  await expect(page.getByText("Ready", { exact: true })).toBeVisible();
  await expect(page.getByText("Voix intégrée", { exact: true })).toBeVisible();
});


test("Assistant audit statuses translate while requests and stored phases remain intact", async ({ page }) => {
  await boot(page, "fr-CA");
  await page.getByRole("navigation", { name: "Principal" }).getByRole("button", { name: "Historique", exact: true }).click();
  await page.getByRole("group", { name: "Type d’historique" }).getByRole("button", { name: "Assistant", exact: true }).click();
  await expect(page.getByText(/Test editor · Résultat/)).toBeVisible();
  await page.getByRole("button", { name: "Voir l’exécution" }).click();
  const record = page.getByRole("article", { name: "Enregistrement de génération" });
  await expect(record.getByText("My English request", { exact: true })).toBeVisible();
  await record.getByText("Chronologie de l’exécution", { exact: true }).click();
  await expect(record.locator("li")).toContainText("Génération · My diagnostic");
  expect(await page.evaluate(() => (window as any).__native("generation_history", { id: "test-run" }).then((run: any) => run.events[0].phase))).toBe("streaming");
});
