import { expect, test, type Locator, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

const fixture = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'tests', 'fixtures', 'diagnostic-response.json'), 'utf8'));

async function noPageOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBeTruthy();
}

async function touchTarget(locator: Locator) {
  const bounds = await locator.boundingBox();
  expect(bounds?.height).toBeGreaterThanOrEqual(44);
  expect(bounds?.width).toBeGreaterThanOrEqual(44);
}

test('phone diagnosis, evidence and Help remain usable and preserve results', async ({ page, isMobile }) => {
  test.skip(!isMobile, 'Touch layout regression');
  const press = (locator: Locator) => locator.tap();
  const response = structuredClone(fixture);
  const longEvidence = `Program evidence: ${'TokenAccountEvidence'.repeat(35)}`;
  response.transaction.logs = [longEvidence];
  response.formatted_text = longEvidence;
  response.transaction.execution_tree[0].depth = 16;
  await page.route('**/api/diagnose', (route) => route.fulfill({ json: response }));
  await page.goto('/');
  const signature = page.getByPlaceholder('Paste a Solana transaction signature');
  await signature.fill(response.transaction.signature);
  expect(await signature.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
  await touchTarget(page.getByRole('button', { name: 'Debug', exact: true }));
  await press(page.getByRole('button', { name: 'Debug', exact: true }));
  await expect(page.getByRole('heading', { name: 'Diagnosis', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Diagnostic results' })).toBeFocused();
  await noPageOverflow(page);
  const views = page.getByRole('group', { name: 'Transaction evidence views' });
  for (const name of ['Execution Tree', 'Compute + Fees', 'Accounts', 'Logs', 'Raw']) {
    const button = views.getByRole('button', { name, exact: true });
    await touchTarget(button);
    await press(button);
    await expect(button).toHaveAttribute('aria-pressed', 'true');
    await noPageOverflow(page);
  }
  await expect(page.getByRole('heading', { name: 'JSON', exact: true })).toBeVisible();
  await page.setViewportSize({ width: 844, height: 390 });
  await noPageOverflow(page);
  await page.setViewportSize({ width: 320, height: 740 });
  const help = page.getByRole('link', { name: 'Updates', exact: true });
  await touchTarget(help);
  await press(help);
  await page.getByLabel('Product', { exact: true }).selectOption('CLMM');
  const deployment = page.getByLabel('Deployment evidence');
  await deployment.selectOption('Confirmed deployed');
  // The selected text needs room even on the narrowest phone, not just a fitting box.
  const deploymentWidth = await deployment.evaluate((element) => {
    const select = element as HTMLSelectElement;
    const style = getComputedStyle(select);
    const canvas = document.createElement('canvas');
    const context = canvas.getContext('2d')!;
    context.font = style.font;
    return { available: select.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) - 24, text: context.measureText(select.selectedOptions[0].text).width };
  });
  expect(deploymentWidth.available).toBeGreaterThan(deploymentWidth.text);
  await page.getByLabel('Search updates').fill('AccountLack');
  await expect(page.getByRole('status')).toHaveText('1 of 13 updates match your filters');
  const details = page.locator('#update-clmm-freezing summary');
  await touchTarget(details);
  await press(details);
  await expect(page.getByText(/Old close builders/)).toBeVisible();
  await noPageOverflow(page);
  await press(page.getByRole('link', { name: 'Debug', exact: true }));
  await expect(signature).toHaveValue(response.transaction.signature);
  await expect(views.getByRole('button', { name: 'Raw', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('heading', { name: 'JSON', exact: true })).toBeVisible();
});

test('a pending phone diagnosis does not move focus away from Help', async ({ page, isMobile }) => {
  test.skip(!isMobile, 'Touch layout regression');
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  await page.route('**/api/diagnose', async (route) => {
    await pending;
    await route.fulfill({ json: fixture });
  });
  await page.goto('/');
  await page.getByPlaceholder('Paste a Solana transaction signature').fill(fixture.transaction.signature);
  await page.getByRole('button', { name: 'Debug', exact: true }).tap();
  await expect(page.getByRole('button', { name: 'Debug', exact: true })).toBeDisabled();
  await page.getByRole('link', { name: 'Updates', exact: true }).tap();
  const search = page.getByLabel('Search updates');
  await search.fill('CPI');
  await search.focus();
  finish();
  await expect(page.locator('.debug-view .diagnosis-hero')).toHaveCount(1);
  await expect(search).toBeFocused();
  await expect(page.getByRole('heading', { name: 'Developer updates', exact: true })).toBeVisible();
  await page.getByRole('link', { name: 'Debug', exact: true }).tap();
  await expect(page.getByRole('heading', { name: 'Diagnosis', exact: true })).toBeVisible();
});

test('phone loading and API errors recover without losing the signature', async ({ page, isMobile }) => {
  test.skip(!isMobile, 'Touch layout regression');
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  await page.route('**/api/diagnose', async (route) => {
    await pending;
    await route.fulfill({ status: 503, json: { error: `RPC unavailable: ${'endpoint-detail-'.repeat(30)}` } });
  });
  await page.goto('/');
  const signature = page.getByPlaceholder('Paste a Solana transaction signature');
  await signature.fill(fixture.transaction.signature);
  const debug = page.getByRole('button', { name: 'Debug', exact: true });
  await debug.tap();
  await expect(debug).toBeDisabled();
  finish();
  await expect(page.locator('.alert')).toContainText('RPC unavailable');
  await expect(debug).toBeEnabled();
  await expect(signature).toHaveValue(fixture.transaction.signature);
  await noPageOverflow(page);
});
