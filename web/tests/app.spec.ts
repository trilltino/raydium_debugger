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

test('runs symptom-only investigation and renders evidence-backed context', async ({ page }) => {
  const result = {
    investigation_id: 'investigation-test-1',
    status: 'complete',
    signature: null,
    symptom: 'pool not showing',
    cluster: 'devnet',
    transaction_diagnosis: null,
    transaction_error: null,
    related_incidents: [
      {
        id: 'case-approved-1',
        product: 'raydium_cpmm',
        failure_domain: 'indexing',
        summary: 'Pool exists but is not discoverable yet',
        resolution: 'Check the indexer refresh window and confirm the pool account.',
        symptom_tags: ['pool_visibility'],
        evidence_message_count: 4,
      },
    ],
    recent_observations: [
      {
        source: 'indexer',
        cluster: 'devnet',
        observed_at: 1_790_000_000,
        slot: 123,
        program_id: null,
        instruction: 'pool_refresh',
        error_code: null,
        fingerprint: 'fingerprint-1',
      },
    ],
    evidence: [
      {
        evidence_id: 'investigation-test-1:report',
        evidence_type: 'user_report',
        source_reference: 'investigation-test-1',
        summary: 'pool not showing',
        observed_at: null,
      },
    ],
    unknowns: [],
  };
  const accepted = {
    event_type: 'progress',
    investigation_id: result.investigation_id,
    stage: 'accepted',
    message: 'Investigation accepted',
    result: null,
    error: null,
  };
  const complete = {
    event_type: 'complete',
    investigation_id: result.investigation_id,
    stage: 'complete',
    message: 'Investigation complete',
    result,
    error: null,
  };
  await page.route('**/api/investigate', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'text/event-stream',
      body: `event: progress\ndata: ${JSON.stringify(accepted)}\n\nevent: complete\ndata: ${JSON.stringify(complete)}\n\n`,
    });
  });

  await page.goto('/');
  await page.getByPlaceholder('Describe what went wrong (optional with a signature)').fill('pool not showing');
  await page.getByRole('button', { name: 'Investigate' }).click();
  await expect(page.getByRole('region', { name: 'Investigation result' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Pool exists but is not discoverable yet' })).toBeVisible();
  await expect(page.getByText('Check the indexer refresh window and confirm the pool account.')).toBeVisible();
  await expect(page.getByText('indexer · devnet')).toBeVisible();
  await expect(page.getByText('Investigation complete')).toBeVisible();
});

test('recovers a completed investigation after the SSE stream closes early', async ({ page }) => {
  const result = {
    investigation_id: 'investigation-recovered-1',
    status: 'complete',
    signature: null,
    symptom: 'pool missing',
    cluster: 'devnet',
    transaction_diagnosis: null,
    transaction_error: null,
    related_incidents: [],
    recent_observations: [],
    evidence: [],
    unknowns: ['No approved historical incident matched this input.'],
  };
  await page.route('**/api/investigate', async (route) => {
    const accepted = {
      event_type: 'progress',
      investigation_id: result.investigation_id,
      stage: 'accepted',
      message: 'Investigation accepted',
      result: null,
      error: null,
    };
    await route.fulfill({
      status: 200,
      contentType: 'text/event-stream',
      body: `event: progress\ndata: ${JSON.stringify(accepted)}\n\n`,
    });
  });
  await page.route('**/api/investigations/investigation-recovered-1', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        investigation_id: result.investigation_id,
        status: 'complete',
        result,
      }),
    });
  });

  await page.goto('/');
  await page.getByPlaceholder('Describe what went wrong (optional with a signature)').fill('pool missing');
  await page.getByRole('button', { name: 'Investigate' }).click();
  await expect(page.getByRole('region', { name: 'Investigation result' })).toBeVisible();
  await expect(
    page.getByRole('region', { name: 'Unknowns' }).getByText('No approved historical incident matched this input.'),
  ).toBeVisible();
});

test('shows structured backend errors without mocked data', async ({ page }) => {
  await page.goto('/');
  await page.getByPlaceholder('Paste a Solana transaction signature').fill('not-a-signature');
  await page.getByRole('button', { name: /debug/i }).click();
  await expect(page.getByText(/\[invalid_signature\]/)).toBeVisible();
});

test('indexes signatures by integrator for dropdown reuse', async ({ page }) => {
  const integrator = `Integrator ${Date.now()}-${Math.random().toString(36).slice(2)}`;
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

  test('investigates a signature and symptom through the browser, server, and real RPC', async ({ page, request }) => {
    test.setTimeout(120_000);
    const tx = liveCase('devnet_success');
    await page.goto('/');
    await page.getByPlaceholder('Paste a Solana transaction signature').fill(tx.signature);
    await page.getByLabel('Cluster').selectOption(tx.cluster);
    await page.getByLabel('Support symptom').fill('pool not showing after successful creation');
    const completeResponse = page.waitForResponse((response) => response.url().endsWith('/api/investigate') && response.request().method() === 'POST');
    await page.getByRole('button', { name: 'Investigate', exact: true }).click();
    const response = await completeResponse;
    expect(response.status()).toBe(200);
    await expect(page.getByRole('region', { name: 'Investigation result', exact: true }).getByText('Investigation complete', { exact: true })).toBeVisible({ timeout: 120_000 });
    // The app cancels its SSE reader after completion; Chromium may discard the
    // response body. Replay the actual delivered cursor from the durable ledger.
    const cursor = await page.evaluate(() => {
      const key = Object.keys(localStorage).find((key) => key.startsWith('raydium-investigation:'));
      return key ? JSON.parse(localStorage.getItem(key)!) as { id: string; after: string } : null;
    });
    expect(cursor).not.toBeNull();
    expect(BigInt(cursor!.after)).toBeGreaterThan(0n);
    const session = await (await request.get('/api/session')).json();
    const headers = { 'x-raydium-debugger-token': session.api_token };
    const replay = await request.get(`/api/investigations/${cursor!.id}/events?after=0`, { headers });
    expect(replay.status()).toBe(200);
    const stream = await replay.text();
    const block = stream.split(/\r?\n/).find((line) => line.startsWith('data:') && line.includes('"event_type":"complete"'));
    expect(block, stream).toBeTruthy();
    const result = JSON.parse(block!.slice(5).trim()).result;
    expect(result.transaction_error).toBeNull();
    expect(result.transaction_diagnosis.observation.status).toBe('landed');
    expect(result.transaction_diagnosis.transaction.signature).toBe(tx.signature);
    expect(result.transaction_diagnosis.transaction.success).toBe(true);
    expect(Number(result.transaction_diagnosis.transaction.slot_exact)).toBeGreaterThan(0);
    expect(result.evidence.some((item: { evidence_type: string }) => item.evidence_type === 'transaction_diagnosis')).toBe(true);
    await expect(page.getByRole('region', { name: 'Investigation result', exact: true })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Evidence ledger' }).getByText(tx.signature, { exact: true })).toBeVisible();
    expect(result.investigation_id).toBe(cursor!.id);
    const lookup = await request.get(`/api/investigations/${result.investigation_id}`, { headers });
    expect((await lookup.json()).result).toEqual(result);
  });

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

    await expect(page.locator('.result').getByRole('heading', { name: 'Token-2022 account needs more funds' })).toBeVisible({
      timeout: 60_000,
    });
    await expect(page.getByText('What To Do Next')).toBeVisible();
    const nextActions = page.locator('section.panel').filter({ has: page.getByRole('heading', { name: 'What To Do Next', exact: true }) });
    await expect(nextActions.getByText(/Find the token account used as the source\/input account/)).toBeVisible();
    await expect(page.getByText('How We Decoded This')).toBeVisible();
    await expect(page.getByText(/Matched code 1 \(0x1\) against the Token-2022 error list/)).toBeVisible();
    await expect(page.getByText('Do not retry blindly')).toHaveCount(0);
  });
});

test('starts an investigation from a real recent observation group without a signature', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('Cluster').selectOption('devnet');
  await page.getByRole('button', { name: 'Browse recent observations' }).click();
  const submission = page.waitForResponse((response) => response.url().endsWith('/api/investigate'));
  await page.getByRole('button', { name: /Investigate indexer.*pool_refresh/ }).click();
  const response = await submission;
  const input = response.request().postDataJSON();
  expect(input.signature).toBeNull();
  expect(input.symptom).toBeNull();
  expect(input.recent_fingerprint).toMatch(/^[0-9a-f]{64}$/);
  expect(input.cluster).toBe('devnet');
  await expect(page.getByRole('heading', { name: 'Recent execution investigation' })).toBeVisible();
  const region = page.getByRole('region', { name: 'Recent observations', exact: true });
  await expect(region.getByText('pool_refresh')).toBeVisible();
  await expect(page.getByText(/private operational detail/)).toHaveCount(0);
});

test('desktop symptom investigation uses the shared command instead of signature diagnosis', async ({ page }) => {
  await page.addInitScript(() => {
    const records: { command: string; args: unknown }[] = [];
    Object.assign(window, { desktopCalls: records, __TAURI_INTERNALS__: {
      transformCallback: () => 1,
      unregisterCallback: () => {},
      invoke: async (command: string, args: { request?: { symptom?: string } }) => {
        records.push({ command, args });
        if (command === 'providers_cmd') return { name: 'triton_one', triton: { devnet_configured: false, mainnet_configured: false } };
        if (command === 'investigate_cmd') return {
          investigation_id: 'desktop-symptom', status: 'partial', signature: null, symptom: args.request?.symptom,
          cluster: 'devnet', transaction_diagnosis: null, transaction_error: null, related_incidents: [], recent_observations: [],
          evidence: [{ evidence_id: 'desktop-symptom:report', evidence_type: 'user_report', source_reference: 'desktop-symptom', summary: args.request?.symptom, observed_at: null }],
          unknowns: ['No transaction signature was supplied; no on-chain failure is asserted.'],
        };
        throw new Error(`Unexpected desktop command: ${command}`);
      },
    } });
  });
  await page.goto('/');
  await page.getByLabel('Support symptom').fill('pool not showing');
  await page.getByRole('button', { name: 'Investigate', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Investigation result', exact: true })).toBeVisible();
  const calls = await page.evaluate(() => (window as unknown as { desktopCalls: { command: string; args: { request?: { symptom?: string; signature?: string | null } } }[] }).desktopCalls);
  const investigation = calls.find((call) => call.command === 'investigate_cmd');
  expect(investigation?.args.request?.symptom).toBe('pool not showing');
  expect(investigation?.args.request?.signature).toBeNull();
  expect(calls.some((call) => call.command === 'debug_transaction_cmd')).toBe(false);
});
