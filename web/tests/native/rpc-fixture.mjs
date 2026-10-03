// Mock only the JSON-RPC network boundary. The actual Rust decoder and IPC execute.
import http from 'node:http';
import fs from 'node:fs';

const signature = Buffer.alloc(64, 31);
const message = Buffer.concat([
  Buffer.from([1, 0, 1, 2]), Buffer.alloc(32, 7), Buffer.alloc(32), Buffer.alloc(32, 2),
  Buffer.from([1, 1, 2, 0, 0, 12, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]),
]);
const transaction = Buffer.concat([Buffer.from([1]), signature, message]).toString('base64');
let failure = false;
let delay = 0;
let requests = 0;

const server = http.createServer(async (request, response) => {
  let raw = '';
  for await (const part of request) raw += part;
  if (request.url === '/control') {
    const control = JSON.parse(raw || '{}');
    failure = control.failure ?? false; delay = control.delay ?? 0;
    response.end(JSON.stringify({ requests })); return;
  }
  requests += 1;
  const payload = JSON.parse(raw);
  if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
  let result;
  if (failure) {
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify({ jsonrpc: '2.0', id: payload.id, error: { code: -32603, message: 'Fixture provider unavailable' } }));
    return;
  }
  switch (payload.method) {
    case 'getTransaction': result = {
      slot: 1000, blockTime: Math.floor(Date.now() / 1000), version: 'legacy',
      transaction: [transaction, 'base64'],
      meta: { err: null, status: { Ok: null }, fee: 5000, preBalances: [10000000, 1], postBalances: [9995000, 1],
        innerInstructions: [], logMessages: ['Program 11111111111111111111111111111111 invoke [1]', 'Program 11111111111111111111111111111111 success'],
        preTokenBalances: [], postTokenBalances: [], rewards: [], computeUnitsConsumed: 150,
        loadedAddresses: { writable: [], readonly: [] } },
    }; break;
    case 'getSignatureStatuses': result = { context: { slot: 1001 }, value: [{ slot: 1000, confirmations: null, err: null, confirmationStatus: 'finalized', status: { Ok: null } }] }; break;
    case 'getSlot': result = 1001; break;
    case 'getVersion': result = { 'solana-core': '3.1.12', 'feature-set': 0 }; break;
    case 'getMultipleAccounts': result = { context: { slot: 1001 }, value: payload.params[0].map(() => null) }; break;
    case 'getAccountInfo': result = { context: { slot: 1001 }, value: null }; break;
    case 'getMinimumBalanceForRentExemption': result = 0; break;
    default: result = null;
  }
  response.setHeader('content-type', 'application/json');
  response.end(JSON.stringify({ jsonrpc: '2.0', id: payload.id, result }));
});
server.listen(0, '127.0.0.1', () => fs.writeFileSync(process.argv[2], JSON.stringify({ port: server.address().port })));
