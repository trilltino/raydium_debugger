import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testIgnore: '**/native/**',
  webServer: {
    command: 'npm run build && node tests/seed-observations.mjs && cd .. && cargo run -p raydium-debugger-server -- --bind 127.0.0.1:8791',
    url: 'http://127.0.0.1:8791/api/health',
    reuseExistingServer: false,
    // Cold Windows builds include the offline ingestion adapter and Solana graph.
    timeout: 600_000,
    env: {
      ...process.env,
      RAYDIUM_DEBUGGER_CASEBOOK_PATH: 'target/playwright-casebooks.sqlite',
      RAYDIUM_DEBUGGER_INVESTIGATION_PATH: 'target/playwright-investigations.sqlite',
      RAYDIUM_DEBUGGER_OBSERVATIONS_DATABASE_PATH: 'target/playwright-observations.sqlite',
      RAYDIUM_DEBUGGER_KNOWLEDGE_PATH: 'target/playwright-incidents.json',
      ...(process.env.RUN_LIVE_E2E === '1' ? {} : {
        TRITON_DEVNET_RPC_URL: 'http://127.0.0.1:9',
        TRITON_MAINNET_RPC_URL: 'http://127.0.0.1:9',
        TRITON_DEVNET_FALLBACK_RPC_URL: '',
        TRITON_MAINNET_FALLBACK_RPC_URL: '',
      }),
    },
  },
  use: {
    baseURL: 'http://127.0.0.1:8791',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile', use: { ...devices['Pixel 5'] } },
    { name: 'mobile-small', use: { ...devices['Pixel 5'], viewport: { width: 320, height: 740 } } },
    { name: 'mobile-safari', use: { ...devices['iPhone 13'] } },
  ],
});
