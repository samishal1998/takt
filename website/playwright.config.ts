import { defineConfig } from '@playwright/test';

const browserName = process.env.PLAYWRIGHT_BROWSER === 'firefox' ? 'firefox' : 'chromium';

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:4321',
    browserName,
    colorScheme: 'light',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1280, height: 900 } } },
    {
      name: 'mobile',
      use: {
        viewport: { width: 390, height: 844 },
        ...(browserName === 'chromium' ? { isMobile: true, hasTouch: true } : {}),
      },
    },
  ],
  webServer: {
    command: 'npm run preview -- --host 127.0.0.1 --ignore-lock',
    url: 'http://127.0.0.1:4321/takt/',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
