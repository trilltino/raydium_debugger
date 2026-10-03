import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  // Help is local content and remains usable when the debugger backend is offline.
  await page.route('**/api/**', (route) => route.abort());
});

test('Help navigation preserves signature and supports history and direct links', async ({ page }) => {
  await page.goto('/');
  const signature = page.getByPlaceholder('Paste a Solana transaction signature');
  await signature.fill('signature-to-preserve');
  await page.getByRole('link', { name: 'Updates', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Developer updates', exact: true })).toBeVisible();
  await expect(page.locator('.alert')).toBeHidden();
  await expect(page.getByRole('link', { name: 'Updates', exact: true })).toHaveAttribute('aria-current', 'page');
  await page.getByRole('link', { name: 'Debug', exact: true }).click();
  await expect(signature).toHaveValue('signature-to-preserve');
  await page.goBack();
  await expect(page.getByRole('heading', { name: 'Developer updates', exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Developer updates', exact: true })).toBeVisible();
});

test('search, product and deployment filters compose and reset', async ({ page }) => {
  await page.goto('/#help');
  await expect(page.getByRole('status')).toHaveText('13 of 13 updates · newest first');
  await page.getByLabel('Product', { exact: true }).selectOption('CLMM');
  await page.getByLabel('Deployment evidence').selectOption('Announcement only');
  await expect(page.getByRole('status')).toHaveText('1 of 13 updates match your filters');
  await page.getByLabel('Search updates').fill('nonexistent-release-term');
  await expect(page.getByRole('heading', { name: 'No matching updates' })).toBeVisible();
  await page.getByRole('button', { name: 'Show all updates' }).click();
  await expect(page.getByRole('status')).toHaveText('13 of 13 updates · newest first');
  await expect(page.getByRole('button', { name: 'Reset filters' })).toBeDisabled();
  await page.getByLabel('Search updates').fill('AccountLack');
  await expect(page.getByRole('status')).toHaveText('1 of 13 updates match your filters');
  await expect(page.getByRole('heading', { name: 'CLMM: restricted-asset position NFTs may be frozen' })).toBeVisible();
  await page.getByRole('button', { name: 'Reset filters' }).click();
  await expect(page.getByLabel('Search updates')).toHaveValue('');
});

test('details explain conditional changes and preserve sources and announcement status', async ({ page }) => {
  await page.goto('/#help');
  const pending = page.locator('#update-clmm-anchor');
  await expect(pending.getByText('Announcement only', { exact: true })).toBeVisible();
  await expect(pending.getByRole('link', { name: 'Announcement #38' })).toHaveAttribute('href', 'https://t.me/RaydiumDeveloperUpdates/38');
  await expect(pending.getByText(/Confirm the deployed program before using/)).toBeVisible();
  const frozen = page.locator('#update-clmm-freezing');
  await expect(frozen.getByText(/Old close builders/)).toBeHidden();
  const summary = frozen.locator('summary');
  await summary.focus();
  await page.keyboard.press('Enter');
  await expect(frozen.getByText(/Old close builders/)).toBeVisible();
  await expect(frozen.getByRole('heading', { name: 'What to check in the debugger' })).toBeVisible();
  await expect(frozen.getByRole('link', { name: 'Changelog' })).toHaveAttribute('href', /2026-08-17-clmm-restricted-position-nft-freeze/);
  await expect(frozen.getByText('17 August 2026', { exact: true })).toBeVisible();
  await expect(page.getByRole('main')).toHaveCount(1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy();
});
