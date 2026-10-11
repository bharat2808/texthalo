import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';

const { version } = JSON.parse(readFileSync(new URL('../../package.json', import.meta.url), 'utf8'));

test('third-party licenses are discoverable, localized, and readable on mobile', async ({ page }) => {
  await page.route('**/*.neonauth.*/**', route => route.abort());
  await page.goto('/fr-CA/');
  await page.locator('footer').getByRole('link', { name: 'Licences de tiers' }).click();
  await expect(page).toHaveURL(/\/fr-CA\/licenses\/$/);
  await expect(page.locator('h1')).toHaveText('Licences de tiers');
  await expect(page.locator('main')).toContainText('Kokoro-82M');
  await expect(page.locator('main')).toContainText('NAIST-JDIC');
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  const inventory = await page.request.get('/licenses/dependencies.json');
  expect(inventory.ok()).toBe(true);
  expect((await inventory.json()).version).toBe(version);
  const notice = await page.request.get('/licenses/attributions/espeak-ng-COPYING.txt');
  expect(notice.ok()).toBe(true);
  expect(await notice.text()).toContain('GNU GENERAL PUBLIC LICENSE');
  await page.locator('.website-language-picker select').selectOption('ja');
  await expect(page).toHaveURL(/\/ja\/licenses\/$/);
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
});

test('licenses and original notices are accessible without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('/licenses/');
  await expect(page.locator('h1')).toHaveText('Third-party licenses');
  await page.locator('main').getByRole('link', { name: 'kokoro-Apache-2.0.txt' }).click();
  await expect(page.locator('body')).toContainText('Apache License');
  await context.close();
});
