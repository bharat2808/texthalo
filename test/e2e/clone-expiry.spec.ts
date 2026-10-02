import { expect, test, type Page } from "@playwright/test";

async function openDesktop(page: Page, failExpiredList = false) {
  await page.clock.install({ time: new Date("2026-10-02T12:00:00Z") });
  await page.addInitScript(({ failExpiredList }) => {
    const fixture = {
      expiresAt: Date.now() + 60_000,
      cloneReads: 0,
      saves: [] as Record<string, any>[],
      state: {
        settings: {
          accessibility_prompted: true, onboarding_completed: true, launch_at_login: false,
          shortcuts: { speak: "Alt+Space", stop: "Alt+Escape" }, engine: "fish",
          kokoro: { voice: "af_heart", quant: "q8", speed: 1, keep_warm: false, idle_unload_minutes: 5 },
          chatterbox: { voice: "en", exaggeration: 0.5, cfg_weight: 0.5, ref_audio: null, keep_warm: false },
          fish: { voice_id: "", model_id: "fish-s21-pro", enhance_text: false, privacy_accepted: true },
          voice: null, rate: 200, capture_mode: "ax_then_copy", max_chars: 10_000, restore_clipboard: true,
        },
        engines: [{ id: "fish", label: "Fish Audio", summary: "Hosted speech", can_speak: true,
          status: "Ready", blocked_reason: null, needs_download: false, download_bytes: 0, repo: "",
          voices: [], ref_voices: [], selected_voice: "" }],
        voices: [], trusted: true, secure_input: false, speaking: false, refused_shortcuts: [],
        system_language: "en_CA", config_path: "/test/settings.json",
      },
    };
    Object.assign(window, { __desktopFixture: fixture, __testInvoke: async (command: string, args: any) => {
      const expired = Date.now() >= fixture.expiresAt;
      switch (command) {
        case "get_state": return structuredClone(fixture.state);
        case "save_settings":
          fixture.state.settings = args.settings;
          fixture.saves.push(structuredClone(args.settings));
          return structuredClone(fixture.state);
        case "plugin:app|version": return "0.1.10";
        case "desktop_is_signed_in": return true;
        case "desktop_account": return {
          availableCredits: 30_000, accountEmail: "test@example.test", billingEnabled: true,
          creditBreakdown: { totalCredits: 30_000, planCredits: 0, topupCredits: 30_000, otherCredits: 0 },
          subscription: null, plans: [], cloneEntitlement: expired ? { limit: 0, source: null, expiresAt: null }
            : { limit: 2, source: "topup_trial", expiresAt: new Date(fixture.expiresAt).toISOString() },
        };
        case "desktop_voice_languages": return { items: [{ code: "en", voiceCount: 1 }] };
        case "desktop_voices": return { items: [], hasMore: false, modelId: "fish-s21-pro" };
        case "desktop_clones":
          fixture.cloneReads++;
          if (expired && failExpiredList) throw new Error("Clone service temporarily unavailable");
          return { items: [{ id: "clone-1", name: "My test voice", status: "trained",
            createdAt: "2026-10-01T12:00:00Z", access: expired ? "disabled" : "active" }] };
        case "desktop_billing_plans": return { plans: [] };
        default: throw new Error(`Unexpected native command: ${command}`);
      }
    } });
  }, { failExpiredList });
  // Run the real app in Chromium with Tauri's official IPC mock at the native boundary.
  await page.route("**/src/main.tsx", async (route) => {
    await route.fulfill({ contentType: "application/javascript", body: `
      import { mockIPC } from "/node_modules/@tauri-apps/api/mocks.js";
      mockIPC(window.__testInvoke, { shouldMockEvents: true });
      await import("/src/main.tsx?e2e");
    ` });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Voice", exact: true }).click();
  const clone = page.getByRole("button", { name: /My test voice/ });
  await expect(clone).toBeVisible();
  await expect(clone).not.toHaveAttribute("aria-disabled", "true");
  return clone;
}

test("trial expiry refreshes clone access without leaving Voice and clears the selected clone", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const clone = await openDesktop(page);
  await clone.click();
  await expect(clone).toHaveAttribute("aria-pressed", "true");
  const reads = await page.evaluate(() => (window as any).__desktopFixture.cloneReads);
  await page.clock.fastForward(61_000);
  await expect(clone).toHaveAttribute("aria-disabled", "true");
  await expect(clone).toHaveAttribute("aria-pressed", "false");
  await expect(clone).toContainText("Stored but inactive");
  await expect.poll(() => page.evaluate(() => (window as any).__desktopFixture.cloneReads)).toBeGreaterThan(reads);
  await expect.poll(() => page.evaluate(() => (window as any).__desktopFixture.state.settings.fish.voice_id)).toBe("");
  const saves = await page.evaluate(() => (window as any).__desktopFixture.saves.length);
  await clone.dispatchEvent("click");
  await clone.dispatchEvent("keydown", { key: "Enter" });
  expect(await page.evaluate(() => (window as any).__desktopFixture.saves.length)).toBe(saves);
  expect(errors).toEqual([]);
});

test("expired entitlement disables cached clones even when the list refresh fails", async ({ page }) => {
  const clone = await openDesktop(page, true);
  await page.clock.fastForward(61_000);
  await expect(clone).toHaveAttribute("aria-disabled", "true");
  await expect(clone).toContainText("Stored but inactive");
  const saves = await page.evaluate(() => (window as any).__desktopFixture.saves.length);
  await clone.dispatchEvent("click");
  await clone.dispatchEvent("keydown", { key: "Enter" });
  expect(await page.evaluate(() => (window as any).__desktopFixture.saves.length)).toBe(saves);
});
