// Isolated operational fixtures: no support archive or provider credentials.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const root = path.resolve('..');
const input = path.join(root, 'target', 'playwright-observation-input.json');
fs.mkdirSync(path.dirname(input), { recursive: true });
fs.writeFileSync(input, JSON.stringify([{
  source: 'indexer', source_id: 'playwright-pool-visibility', cluster: 'devnet',
  observed_at: Math.floor(Date.now() / 1000), slot: 123,
  instruction: 'pool_refresh', error_code: null,
  logs: ['pool not showing; private operational detail excluded from public results'],
}]));
execFileSync('cargo', ['run', '-p', 'xtask', '--', 'support-knowledge', 'observations', 'ingest-json', input, 'target/playwright-observations.sqlite'], { cwd: root, stdio: 'inherit' });
