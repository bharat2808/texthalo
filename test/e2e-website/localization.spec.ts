import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/*.neonauth.*/**", route => route.abort());
  await page.route("**/v1/billing/plans", route => route.fulfill({ json: { billingEnabled: true, plans: [
    { id: "free", name: "Free", description: "A calm place to start listening.", price: { unitAmount: 0, currency: "usd", interval: null }, creditsPerPeriod: 0, cloneLimit: 0, features: [], displayOrder: 0 },
    { id: "plus", name: "Plus", description: "Hosted voices when you want a faster path.", price: { unitAmount: 2345, currency: "cad", interval: "month" }, creditsPerPeriod: 123456, cloneLimit: 2, features: ["Everything in Free", "123,456 hosted credits each month", "Up to 7 saved Fish Audio clones"], displayOrder: 1 },
  ], topups: [{ id: "live-pack", priceCents: 750, credits: 60000 }] } }));
});

test("French browser automatically uses a localized, hydration-safe page", async ({ browser }) => {
  const context = await browser.newContext({ locale: "fr-CA" });
  const page = await context.newPage();
  await page.route("**/*.neonauth.*/**", route => route.abort());
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/");
  await expect(page).toHaveURL(/\/fr-CA\/$/);
  await expect(page.locator("html")).toHaveAttribute("lang", "fr-CA");
  await expect(page.locator("h1")).toContainText("Vos mots.");
  await expect(page.locator(".website-language-picker select")).toHaveValue("fr-CA");
  expect(errors).toEqual([]);
  await context.close();
});

test("manual English persists across navigation and overrides French browser preferences", async ({ browser }) => {
  const context = await browser.newContext({ locale: "fr-CA" });
  const page = await context.newPage();
  await page.route("**/*.neonauth.*/**", route => route.abort());
  await page.goto("/fr-CA/pricing/");
  await page.locator(".website-language-picker select").selectOption("en");
  await expect(page).toHaveURL(/\/pricing\/$/);
  await expect(page.locator("h1")).toContainText("Start free.");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await page.goto("/privacy/");
  await expect(page.locator("h1")).toHaveText("Privacy policy");
  await context.close();
});

test("explicit Spanish URL wins over saved language and preserves connect parameters on switching", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("texthalo.website.language", "fr-CA"));
  await page.goto("/es-419/desktop-connect/?state=keep-me&code_challenge=keep-this#continue");
  await expect(page.locator("html")).toHaveAttribute("lang", "es-419");
  await page.locator(".website-language-picker select").selectOption("fr-CA");
  await expect(page).toHaveURL(/\/fr-CA\/desktop-connect\/\?state=keep-me&code_challenge=keep-this#continue$/);
});

test("live plan data uses translated descriptions, backend limits, and localized financial formatting", async ({ page }) => {
  await page.goto("/fr-CA/pricing/");
  const plus = page.locator(".plan-card").filter({ has: page.locator(".plan-name", { hasText: "Plus" }) });
  await expect(plus.locator(".plan-price")).toContainText("23,45");
  await expect(plus.locator(".plan-price")).toContainText("mois");
  await expect(plus).toContainText("123 456 crédits hébergés par mois");
  await expect(plus).toContainText("Jusqu’à 2 voix Fish Audio clonées enregistrées");
  await expect(plus.locator(".plan-description")).toContainText("Des voix hébergées");
});

test("unsupported browser language falls back to English", async ({ browser }) => {
  const context = await browser.newContext({ locale: "ja-JP" });
  const page = await context.newPage();
  await page.route("**/*.neonauth.*/**", route => route.abort());
  await page.goto("/");
  await expect(page.locator("h1")).toContainText("Your words.");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await context.close();
});

test("French editorial content and SEO are present without JavaScript", async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto("/fr-CA/blog/how-to-read-text-aloud-on-mac/");
  await expect(page.locator("h1")).toHaveText("Comment faire lire du texte sélectionné à votre Mac");
  await expect(page.locator(".article-body")).toContainText("Choisissez une voix");
  await expect(page).toHaveTitle("Comment faire lire du texte sélectionné à votre Mac | TextHalo");
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute("href", "https://texthalo.app/fr-CA/blog/how-to-read-text-aloud-on-mac/");
  await expect(page.locator('link[hreflang="es-419"]')).toHaveAttribute("href", "https://texthalo.app/es-419/blog/how-to-read-text-aloud-on-mac/");
  await context.close();
});

test("language controls and localized navigation fit a narrow screen", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/fr-CA/pricing/");
  await expect(page.locator(".website-language-picker select")).toBeVisible();
  await page.getByRole("button", { name: "Ouvrir le menu" }).click();
  await expect(page.getByRole("navigation").getByRole("link", { name: "Tarifs" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});


test("English selection works when browser storage is disabled", async ({ browser }) => {
  const context = await browser.newContext({ locale: "fr-CA" });
  const page = await context.newPage();
  await page.addInitScript(() => {
    Storage.prototype.setItem = () => { throw new Error("Storage disabled"); };
    Storage.prototype.getItem = () => { throw new Error("Storage disabled"); };
  });
  await page.route("**/*.neonauth.*/**", route => route.abort());
  await page.goto("/fr-CA/");
  await page.locator(".website-language-picker select").selectOption("en");
  await expect(page).toHaveURL(/\/en\/$/);
  await expect(page.locator("h1")).toContainText("Your words.");
  await page.getByRole("navigation").getByRole("link", { name: "Pricing", exact: true }).click();
  await expect(page).toHaveURL(/\/en\/pricing\/$/);
  await expect(page.locator("h1")).toContainText("Start free.");
  await context.close();
});

test("translated headers fit desktop and mobile editorial layouts", async ({ page }) => {
  for (const [width, route] of [[1280, "/fr-CA/pricing/"], [390, "/es-419/privacy/"]] as const) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto(route);
    const fits = await page.locator(".site-header").evaluate(header => {
      const box = header.getBoundingClientRect();
      const elements = [header.querySelector(".wordmark"), header.querySelector("select"), ...header.querySelectorAll("nav a"), header.querySelector(".header-signin")].filter((element): element is Element => Boolean(element));
      const visible = elements.map(element => element.getBoundingClientRect()).filter(rect => rect.width > 0);
      return visible.every(rect => rect.left >= box.left - 1 && rect.right <= box.right + 1 && rect.top >= box.top - 1 && rect.bottom <= box.bottom + 1);
    });
    expect(fits).toBe(true);
  }
});
