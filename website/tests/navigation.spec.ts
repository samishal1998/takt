import { expect, test, type Page } from '@playwright/test';

const browserFailures = new WeakMap<Page, string[]>();

test.beforeEach(({ page, baseURL }) => {
  const failures: string[] = [];
  browserFailures.set(page, failures);
  const origin = new URL(baseURL!).origin;
  page.on('pageerror', (error) => failures.push(`Browser error: ${error.message}`));
  page.on('response', (response) => {
    if (new URL(response.url()).origin === origin && response.status() >= 400) {
      failures.push(`HTTP ${response.status()}: ${response.url()}`);
    }
  });
});

test.afterEach(({ page }) => {
  expect(browserFailures.get(page), 'Pages should load without browser errors or missing resources')
    .toEqual([]);
});

async function expectNoHorizontalOverflow(page: Page) {
  const excess = await page.evaluate(() =>
    document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(excess, 'The page should fit the viewport horizontally').toBeLessThanOrEqual(1);
}

test('landing page leads to the quick start', async ({ page }, testInfo) => {
  await page.goto('/takt/');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Schedule. Balance. Flow.');
  await expectNoHorizontalOverflow(page);
  await page.screenshot({ path: testInfo.outputPath('landing.png'), fullPage: true });

  await page.getByRole('link', { name: 'Get started', exact: true }).click();
  await expect(page).toHaveURL(/\/takt\/quick-start\/$/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Quick start');
  await expect(page.getByRole('heading', { name: 'Save a workflow' })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});

test('generated guides retain working links under the GitHub Pages base path', async ({ page }) => {
  await page.goto('/takt/installation/');
  await expect(page.getByRole('heading', { level: 1 }))
    .toHaveText('Install Takt or use the project wrapper');
  await expect(page.locator('main')).toContainText('TAKT_USE_SYSTEM=0');
  await page.getByRole('link', { name: 'manual installation', exact: true }).click();
  await expect(page).toHaveURL(/\/takt\/release\/#install-a-published-archive$/);
  await expect(page.getByRole('heading', { name: 'Install a published archive' })).toBeVisible();

  await page.goto('/takt/configuration/');
  await page.getByRole('link', { name: 'runtime contract', exact: true }).first().click();
  await expect(page).toHaveURL(/\/takt\/runtime\/#lifecycle-and-enforcement$/);
  await expect(page.getByRole('heading', { name: 'Lifecycle and enforcement' })).toBeVisible();
});

test('search finds wrapper settings and opens the installation guide', async ({ page }) => {
  await page.goto('/takt/');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Search', exact: true });
  await expect(dialog).toBeVisible();
  await dialog.getByPlaceholder('Search', { exact: true }).fill('TAKT_USE_SYSTEM');
  const result = dialog.getByRole('link', { name: /Install Takt or use the project wrapper/ }).first();
  await expect(result).toBeVisible();
  await result.click();
  await expect(page).toHaveURL(/\/takt\/installation\/(?:#.*)?$/);
  await expect(page.getByRole('heading', { level: 1 }))
    .toHaveText('Install Takt or use the project wrapper');
});

test('mobile menu opens the command reference without horizontal overflow', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'mobile', 'The desktop layout has a persistent sidebar.');
  await page.goto('/takt/quick-start/');
  await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page.getByRole('navigation', { name: 'Main', exact: true })
    .getByRole('link', { name: 'Command reference', exact: true }).click();
  await expect(page).toHaveURL(/\/takt\/cli\/$/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('CLI reference');
  await expectNoHorizontalOverflow(page);
});

test('theme selection persists after reloading the page', async ({ page }, testInfo) => {
  await page.goto('/takt/quick-start/');
  for (const theme of ['dark', 'light']) {
    if (testInfo.project.name === 'mobile') {
      await page.getByRole('button', { name: 'Menu', exact: true }).click();
    }
    await page.getByRole('combobox', { name: 'Select theme', exact: true }).selectOption(theme);
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
  }
});
