// Native external WebDriver setup, following Tauri's Windows guidance.
import fs from 'node:fs';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const base = path.join(root, 'target/native-tests');
const executable = path.join(root, 'target/debug/raydium-debugger-tauri.exe');
let driver;
let rpc;

async function waitFor(test, message) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    try { if (await test()) return; } catch { /* startup */ }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(message);
}

export const config = {
  runner: 'local', hostname: '127.0.0.1', port: 4444,
  specs: ['./tests/native/investigation.spec.mjs'], maxInstances: 1,
  capabilities: [{ browserName: 'wry', 'wdio:enforceWebDriverClassic': true, 'tauri:options': { application: executable } }],
  logLevel: 'warn', framework: 'mocha', reporters: ['spec'],
  mochaOpts: { timeout: 120000 }, connectionRetryCount: 0,
  before: async () => { await browser.setTimeout({ script: 100000 }); },
  onPrepare: async (_config, capabilities) => {
    if (process.platform !== 'win32') throw new Error('Native verification requires Windows');
    // Every native run gets isolated application and database directories.
    fs.mkdirSync(base, { recursive: true });
    const run = fs.mkdtempSync(path.join(base, 'run-'));
    const application = path.join(run, 'raydium-native-test.exe');
    fs.copyFileSync(executable, application);
    capabilities[0]['tauri:options'].application = application;
    const rpcInfo = path.join(run, 'rpc.json');
    const rpcLog = fs.openSync(path.join(run, 'rpc.log'), 'a');
    rpc = spawn(process.execPath, [path.join(root, 'web/tests/native/rpc-fixture.mjs'), rpcInfo], { windowsHide: true, stdio: ['ignore', rpcLog, rpcLog] });
    fs.closeSync(rpcLog);
    await waitFor(() => fs.existsSync(rpcInfo), 'RPC fixture startup failed');
    const { port } = JSON.parse(fs.readFileSync(rpcInfo));
    Object.assign(process.env, {
      TRITON_DEVNET_RPC_URL: `http://127.0.0.1:${port}`, TRITON_DEVNET_FALLBACK_RPC_URL: '',
      TRITON_MAINNET_RPC_URL: '', TRITON_MAINNET_FALLBACK_RPC_URL: '',
      RAYDIUM_DEBUGGER_APPLICATION_DATA_PATH: run,
      RAYDIUM_DEBUGGER_INVESTIGATION_PATH: path.join(run, 'investigations.sqlite'),
      RAYDIUM_DEBUGGER_OBSERVATIONS_DATABASE_PATH: path.join(run, 'observations.sqlite'),
      RAYDIUM_DEBUGGER_KNOWLEDGE_PATH: path.join(run, 'incidents.generated.json'),
      NATIVE_RPC_CONTROL: `http://127.0.0.1:${port}/control`,
      NATIVE_APPLICATION: application,
    });
    fs.writeFileSync(process.env.RAYDIUM_DEBUGGER_KNOWLEDGE_PATH, JSON.stringify({ incidents: [] }));
    const observation = path.join(run, 'input.json');
    fs.writeFileSync(observation, JSON.stringify([{ source: 'indexer', source_id: 'native-group', cluster: 'devnet', observed_at: Math.floor(Date.now()/1000), instruction: 'pool_refresh', logs: ['pool not showing'] }]));
    execFileSync(path.join(root, 'target/debug/xtask.exe'), ['support-knowledge', 'observations', 'ingest-json', observation, process.env.RAYDIUM_DEBUGGER_OBSERVATIONS_DATABASE_PATH], { cwd: root, stdio: 'inherit' });
    const driverLog = fs.openSync(path.join(run, 'driver.log'), 'a');
    driver = spawn('tauri-driver', ['--native-driver', path.join(root, 'target/native-driver/msedgedriver.exe')], { cwd: root, env: process.env, windowsHide: true, stdio: ['ignore', driverLog, driverLog] });
    fs.closeSync(driverLog);
    await waitFor(async () => (await fetch('http://127.0.0.1:4444/status')).ok, 'tauri-driver startup failed');
  },
  onComplete: () => { driver?.kill(); rpc?.kill(); },
};
