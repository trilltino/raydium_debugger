import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

const fixture = JSON.parse(fs.readFileSync(
  path.join(process.cwd(), 'tests', 'fixtures', 'diagnostic-response.json'), 'utf8',
));

for (const scenario of ['matched', 'empty', 'legacy'] as const) {
  test(`AI answer displays ${scenario} historical guidance`, async ({ page }) => {
    await page.route('**/api/diagnose', (route) => route.fulfill({ json: fixture }));
    await page.route('**/api/ask', (route) => route.fulfill({ json: {
      model: 'test-model', answer: 'Check the transaction evidence first.',
      ...(scenario === 'legacy' ? {} : { knowledge: {
        status: scenario, incident_count: scenario === 'matched' ? 10 : 0,
        incidents: scenario === 'matched' ? [{
          incident_id: 'reviewed-case', summary: 'A reviewed pool visibility incident',
          resolution: 'Check indexer refresh.', strength: 'weak',
          reasons: ['Problem terms matched'], missing_signals: ['Unknown cluster'],
        }] : [],
      } }),
    } }));

    await page.goto('/');
    await page.getByPlaceholder('Paste a Solana transaction signature').fill(fixture.transaction.signature);
    await page.getByRole('button', { name: 'Debug', exact: true }).click();
    await page.getByPlaceholder('Ask about this transaction context').fill('What should I check?');
    await page.getByRole('button', { name: 'Ask', exact: true }).click();
    await expect(page.getByText('Check the transaction evidence first.')).toBeVisible();

    if (scenario === 'matched') {
      await expect(page.getByText('1 historical match from 10 reviewed incidents.')).toBeVisible();
      await page.getByText('reviewed-case · weak match', { exact: true }).click();
      await expect(page.getByText('Check indexer refresh.', { exact: true })).toBeVisible();
      await expect(page.getByText('Still unknown: Unknown cluster', { exact: true })).toBeVisible();
    } else if (scenario === 'empty') {
      await expect(page.getByText('No reviewed historical incidents have been published yet. Current guidance may still be available below.')).toBeVisible();
    } else {
      await expect(page.getByLabel('Historical guidance')).toHaveCount(0);
    }
  });
}
