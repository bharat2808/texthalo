import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";

test("desktop handoff runs once automatically, excludes concurrent calls, and permits explicit retry after failure", async () => {
  const vite = await createServer({ configFile: "vite.website.config.ts", server: { middlewareMode: true, hmr: false, ws: false }, appType: "custom" });
  try {
    const { createDesktopHandoffAttempt } = await vite.ssrLoadModule("/src/billing.tsx");
    assert.equal(typeof createDesktopHandoffAttempt, "function");
    const attempt = createDesktopHandoffAttempt();
    let calls = 0;
    let reject;
    const pending = attempt.run(async () => { calls++; await new Promise((_, fail) => { reject = fail; }); });
    await attempt.run(async () => { calls++; });
    await attempt.run(async () => { calls++; }, true);
    assert.equal(calls, 1);
    reject(new Error("offline"));
    await assert.rejects(pending, /offline/);
    await attempt.run(async () => { calls++; });
    assert.equal(calls, 1, "rerenders must not automatically retry failures");
    await attempt.run(async () => { calls++; }, true);
    assert.equal(calls, 2, "manual retry works");
    await attempt.run(async () => { calls++; }, true);
    assert.equal(calls, 2, "successful handoffs cannot run twice");
  } finally { await vite.close(); }
});
