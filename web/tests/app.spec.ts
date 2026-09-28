import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

const diagnosticFixture = JSON.parse(
  fs.readFileSync(path.join(process.cwd(), 'tests', 'fixtures', 'diagnostic-response.json'), 'utf8'),
);

function liveCase(name: string) {
  const raw = fs.readFileSync(path.join(process.cwd(), '..', 'tests', 'live_signatures.toml'), 'utf8');
  const block = raw
    .split('[[case]]')
    .map((part) => part.trim())
    .find((part) => part.includes(`name = "${name}"`));
  if (!block) throw new Error(`missing live case ${name}`);
  const value = (key: string) => block.match(new RegExp(`${key} = "([^"]+)"`))?.[1];
  return {
    signature: value('signature')!,
    cluster: value('cluster')!,
    requiredCode: value('required_code'),
  };
}

test('serves the real app shell and health endpoint', async ({ page, request }) => {
  const health = await request.get('/api/health');
  expect(health.ok()).toBeTruthy();

  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Raydium Debugger' })).toBeVisible();
  await expect(page.getByText('Raydium tooling')).toHaveCount(0);
  await expect(page.getByText('Deterministic failure evidence for Raydium and Solana integrations.')).toHaveCount(0);
  await expect(page.getByText('Live')).toHaveCount(0);
  await expect(page.getByText('Triton RPC')).toHaveCount(0);
  await expect(page.getByText('triton_one')).toHaveCount(0);
  await expect(page.getByText('not configured')).toHaveCount(0);
  await expect(page.getByText('Ready for transaction debugging')).toHaveCount(0);
  await expect(page.getByText('Enter a signature and choose the Triton-backed cluster to inspect live on-chain data.')).toHaveCount(0);
  await expect(page.getByText(/rpcpool\.com/)).toHaveCount(0);

  const favicon = await request.get('/favicon.ico');
  expect(favicon.ok()).toBeTruthy();
});

test('renders mocked v2 diagnosis, execution tree, and compute evidence', async ({ page }) => {
  await page.route('**/api/diagnose', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify(diagnosticFixture),
    });
  });

  await page.goto('/');
  await page.getByPlaceholder('Paste a Solana transaction signature').fill(diagnosticFixture.transaction.signature);
  await page.getByRole('button', { name: /debug/i }).click();

  await expect(page.getByRole('heading', { name: 'Diagnosis', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /copy diagnosis/i })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Raydium CPMM swap failed in Token-2022 CPI' })).toBeVisible();

  await page.getByRole('button', { name: 'Execution Tree' }).click();
  await expect(page.getByRole('heading', { name: 'Execution Tree' })).toBeVisible();
  await expect(page.getByText('transferChecked')).toBeVisible();
  await expect(page.getByText(/Compute: 9,000 \/ 200,000 CU/)).toBeVisible();

  await page.getByRole('button', { name: 'Compute + Fees' }).click();
  await expect(page.getByText('Current fetched account-data bytes')).toBeVisible();
  await expect(page.getByText('Serialized transaction size', { exact: true })).toBeVisible();
});

test('renders non-observed diagnosis as a result, not an error', async ({ page }) => {
  await page.route('**/api/diagnose', async (route) => {
    const response = {
      observation: {
        status: 'not_observed_on_selected_provider',
        cluster: 'mainnet',
        providers_queried: ['configured Triton mainnet endpoint'],
        evidence: ['transaction was not found on the selected cluster/RPC endpoint'],
        hypotheses: ['The transaction was never submitted.'],
      },
      diagnosis: {
        title: 'Transaction was not observed on the selected provider',
        explanation: 'The debugger could not fetch a landed transaction for this signature.',
        primary_action: 'Verify the cluster and retry with submission telemetry.',
        evidence: ['transaction was not found on the selected cluster/RPC endpoint'],
        confidence: 'medium',
        category: 'not_observed',
        copy_markdown: '### Transaction was not observed on the selected provider',
      },
      transaction: null,
      formatted_text: '',
    };
    await route.fulfill({ contentType: 'application/json', body: JSON.stringify(response) });
  });

  await page.goto('/');
  await page.getByPlaceholder('Paste a Solana transaction signature').fill('5U6mZgQmFakeButRouted111111111111111111111111111111111111111111111111111111111111111');
  await page.getByRole('button', { name: /debug/i }).click();

  await expect(page.getByRole('heading', { name: 'Diagnosis', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Transaction was not observed on the selected provider' })).toBeVisible();
  await expect(page.locator('.alert')).toHaveCount(0);
});

test('shows structured backend errors without mocked data', async ({ page }) => {
  await page.goto('/');
  await page.getByPlaceholder('Paste a Solana transaction signature').fill('not-a-signature');
  await page.getByRole('button', { name: /debug/i }).click();
  await expect(page.getByText(/\[invalid_signature\]/)).toBeVisible();
});

test('indexes signatures by integrator for dropdown reuse', async ({ page }) => {
  const integrator = `Integrator ${Date.now()}`;
  const signature = 'ACaZMGWN11hw6ynimi1yCywu5iXmhwk3NWzE7LiE16i1eQo7tECKiSHuSx5iZXqMEt8q2Ryr9zevQEvVeGQR8Ua';

  await page.goto('/');
  await page.getByPlaceholder('Integrator name').fill(integrator);
  await page.getByRole('button', { name: 'Add integrator' }).click();
  await expect(page.getByLabel('Integrator select')).toHaveValue(/.+/);

  await page.getByPlaceholder('Paste a Solana transaction signature').fill(signature);
  await page.getByRole('button', { name: 'Save signature' }).click();
  await expect(page.getByLabel('Saved signature select')).toContainText('Saved transaction');

  await page.getByPlaceholder('Paste a Solana transaction signature').fill('');
  const savedOption = page.getByLabel('Saved signature select').locator('option', { hasText: signature });
  const savedValue = await savedOption.getAttribute('value');
  expect(savedValue).toBeTruthy();
  await page.getByLabel('Saved signature select').selectOption(savedValue!);
  await expect(page.getByPlaceholder('Paste a Solana transaction signature')).toHaveValue(signature);
});

test('shows integrator store validation errors', async ({ page }) => {
  await page.goto('/');
  await page.getByPlaceholder('Integrator name').fill(`Integrator ${Date.now()}`);
  await page.getByRole('button', { name: 'Add integrator' }).click();
  await page.getByPlaceholder('Paste a Solana transaction signature').fill('not-a-signature');
  await page.getByRole('button', { name: 'Save signature' }).click();
  await expect(page.getByText(/\[integrator_store\]/)).toBeVisible();
  await expect(page.getByText(/invalid Solana transaction signature/)).toBeVisible();
});

test.describe('live RPC e2e', () => {
  test.skip(process.env.RUN_LIVE_E2E !== '1', 'Set RUN_LIVE_E2E=1 to hit live Solana RPC.');

  test('debugs a real devnet transaction', async ({ page }) => {
    const tx = liveCase('devnet_success');
    await page.goto('/');
    await page.getByPlaceholder('Paste a Solana transaction signature').fill(tx.signature);
    await page.getByRole('button', { name: /debug/i }).click();
    await expect(page.getByText('Transaction Context')).toBeVisible({ timeout: 60_000 });
    await expect(page.locator('.result').getByText('Landed', { exact: true })).toBeVisible();
  });

  test('returns real evidence for a recent mainnet failure', async ({ page }) => {
    const tx = liveCase('mainnet_unknown_custom');
    await page.goto('/');
    await page.getByPlaceholder('Paste a Solana transaction signature').fill(tx.signature);
    await page.getByLabel('Cluster').selectOption(tx.cluster);
    await page.getByRole('button', { name: /debug/i }).click();
    const decodeStatus = page.getByRole('region', { name: 'Decode status' });
    await expect(decodeStatus.getByText(tx.requiredCode ?? '0x1780', { exact: true })).toBeVisible({
      timeout: 60_000,
    });
    await expect(decodeStatus.getByRole('heading', { name: 'This is a program-specific custom error' })).toBeVisible();
    await expect(page.getByText('For integrator')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Copy handoff' })).toBeVisible();
  });

  test('explains a Token-2022 insufficient funds failure in plain language', async ({ page }) => {
    const tx = liveCase('token_2022_insufficient_funds');
    await page.goto('/');
    await page.getByPlaceholder('Paste a Solana transaction signature').fill(tx.signature);
    await page.getByLabel('Cluster').selectOption(tx.cluster);
    await page.getByRole('button', { name: /debug/i }).click();

    await expect(page.getByRole('heading', { name: 'Token-2022 account needs more funds' })).toBeVisible({
      timeout: 60_000,
    });
    await expect(page.getByText('What To Do Next')).toBeVisible();
    await expect(page.getByRole('listitem').filter({ hasText: /Find the token account used as the source\/input account/ })).toBeVisible();
    await expect(page.getByText('How We Decoded This')).toBeVisible();
    await expect(page.getByText(/Matched code 1 \(0x1\) against the Token-2022 error list/)).toBeVisible();
    await expect(page.getByText('Do not retry blindly')).toHaveCount(0);
  });
});
