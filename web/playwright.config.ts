import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  webServer: {
    command: 'npm run build && cd .. && cargo run -p raydium-debugger-server -- --bind 127.0.0.1:8791',
    url: 'http://127.0.0.1:8791/api/health',
    reuseExistingServer: false,
    timeout: 120_000,
    env: {
      ...process.env,
      RAYDIUM_DEBUGGER_CASEBOOK_PATH: 'target/playwright-casebooks.sqlite',
      TRITON_DEVNET_RPC_URL: process.env.TRITON_DEVNET_RPC_URL ?? 'http://127.0.0.1:9',
      TRITON_MAINNET_RPC_URL: process.env.TRITON_MAINNET_RPC_URL ?? 'http://127.0.0.1:9',
    },
  },
  use: {
    baseURL: 'http://127.0.0.1:8791',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile', use: { ...devices['Pixel 5'] } },
  ],
});
