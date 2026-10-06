import {defineConfig, devices} from '@playwright/test';
export default defineConfig({testDir:'./test/e2e',fullyParallel:false,workers:1,use:{baseURL:'http://127.0.0.1:1423',trace:'retain-on-failure'},projects:[{name:'chromium',use:{...devices['Desktop Chrome']}}],webServer:{command:'npm run dev -- --host 127.0.0.1 --port 1423',url:'http://127.0.0.1:1423',reuseExistingServer:false}});
