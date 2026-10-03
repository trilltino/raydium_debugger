import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';

// 64-byte fixture signature; the network fixture returns the matching binary transaction.
const signature = 'd69ZJoUyxL1FXgkWtDz63ane8kW2t8jfcUg7uAfne4JxDbyscQ2trLiboUu8QHs1EofG6JiVeAmE4jBHfq9jomY';

async function ipc(command, payload) {
  const response = await browser.executeAsync((command, payload, done) => {
    window.__TAURI_INTERNALS__.invoke(command, payload).then((value) => done({ value }), (error) => done({ error: String(error) }));
  }, command, payload);
  if (response.error) throw new Error(response.error);
  return response.value;
}
async function investigate(request) {
  return ipc('investigate_cmd', { request: { cluster: 'devnet', ...request }, onProgress: '__CHANNEL__:0' });
}
async function control(value) {
  return (await fetch(process.env.NATIVE_RPC_CONTROL, { method: 'POST', body: JSON.stringify(value) })).json();
}

describe('built Windows Tauri application and shared Rust service', () => {
  before(async () => { await browser.waitUntil(async () => browser.execute(() => Boolean(window.__TAURI_INTERNALS__)), { timeout: 20000 }); });

  it('runs symptom-only through the actual UI and IPC', async () => {
    const textarea = await $('input[placeholder="Describe what went wrong (optional with a signature)"]');
    await textarea.setValue('pool not showing');
    await $('button=Investigate').click();
    await browser.waitUntil(async () => (await $('body').getText()).includes('Investigation complete'), { timeout: 20000 });
    assert.match(await $('body').getText(), /No approved historical incident/);
  });

  it('fetches signature-only and combined evidence through the network boundary', async () => {
    await control({});
    for (const request of [{ signature }, { signature, symptom: 'pool not showing' }]) {
      const result = await investigate(request);
      assert.equal(result.status, 'complete');
      assert.equal(result.transaction_error, null);
      assert.equal(result.transaction_diagnosis.transaction.slot, 1000);
      assert(result.evidence.some((item) => item.evidence_type === 'transaction_diagnosis'));
    }
  });

  it('starts from a real recent group and replays string event IDs', async () => {
    const groups = await ipc('recent_groups_cmd', { cluster: 'devnet' });
    assert.equal(groups.length, 1);
    const result = await investigate({ recent_fingerprint: groups[0].fingerprint });
    assert.equal(result.recent_observations[0].source, 'indexer');
    const events = await ipc('investigation_events_cmd', { id: result.investigation_id, after: '0' });
    assert(events.every((event) => typeof event.event_id === 'string'));
    assert.equal(events.filter((event) => event.event_type === 'complete').length, 1);
    const last = events.at(-1).event_id;
    assert.deepEqual(await ipc('investigation_events_cmd', { id: result.investigation_id, after: last }), []);
  });

  it('reports provider failure and recovers completed results after application restart', async () => {
    await control({ failure: true });
    const failed = await investigate({ signature });
    assert.equal(failed.status, 'partial');
    assert.equal(failed.transaction_diagnosis, null);
    assert.match(failed.transaction_error, /could not be fetched/);
    await control({});
    const complete = await investigate({ symptom: 'pool visibility' });
    await browser.reloadSession();
    await browser.waitUntil(async () => browser.execute(() => Boolean(window.__TAURI_INTERNALS__)), { timeout: 20000 });
    const recovered = await ipc('investigation_lookup_cmd', { id: complete.investigation_id });
    assert.deepEqual(recovered.result, complete);
  });

  it('interrupts real in-flight work on process termination and retries only explicitly', async () => {
    await control({ delay: 30000 });
    await browser.execute((signature) => {
      window.__TAURI_INTERNALS__.invoke('investigate_cmd', { request: { signature, cluster: 'devnet' }, onProgress: '__CHANNEL__:0' }).catch(() => undefined);
    }, signature);
    // Read only our isolated ledger to discover the newly accepted run. This is
    // test observation, not an IPC replacement or an injected investigation result.
    let id;
    await browser.waitUntil(async () => {
      id = execFileSync('py', ['-3', '-c', 'import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); r=c.execute("SELECT investigation_id FROM investigations WHERE status=\'running\' ORDER BY created_at DESC LIMIT 1").fetchone(); print(r[0] if r else "")', process.env.RAYDIUM_DEBUGGER_INVESTIGATION_PATH], { encoding: 'utf8' }).trim();
      return Boolean(id);
    }, { timeout: 10000 });
    // Terminate only the application launched by this isolated WebDriver session.
    execFileSync('powershell', ['-NoProfile', '-Command', `$nativeExecutable = '${process.env.NATIVE_APPLICATION.replaceAll("'", "''")}'; Get-Process | Where-Object { $_.Path -eq $nativeExecutable } | Stop-Process -Force`]);
    await control({});
    await browser.reloadSession();
    await browser.waitUntil(async () => browser.execute(() => Boolean(window.__TAURI_INTERNALS__)), { timeout: 20000 });
    const interrupted = await ipc('investigation_lookup_cmd', { id });
    assert.equal(interrupted.status, 'interrupted');
    const events = await ipc('investigation_events_cmd', { id, after: '0' });
    assert.equal(events.filter((event) => event.stage === 'interrupted').length, 1);
    const retried = await ipc('investigation_retry_cmd', { id, onProgress: '__CHANNEL__:0' });
    assert.notEqual(retried.investigation_id, id);
    assert.equal(retried.transaction_error, null);
    assert.equal((await ipc('investigation_lookup_cmd', { id })).status, 'interrupted');
  });
});
