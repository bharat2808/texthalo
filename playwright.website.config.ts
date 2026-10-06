import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./test/e2e-website",
  fullyParallel: true,
  use: { baseURL: "http://127.0.0.1:4173", trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: { command: "npx vite preview --config vite.website.config.ts --host 127.0.0.1 --port 4173", url: "http://127.0.0.1:4173", reuseExistingServer: false },
});
