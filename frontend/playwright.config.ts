import { defineConfig } from '@playwright/test';

const port = process.env.FERROCHAT_E2E_PORT ?? '8091';

export default defineConfig({
	testDir: './e2e',
	timeout: 60000,
	use: { baseURL: process.env.FERROCHAT_URL ?? `http://127.0.0.1:${port}` },
	webServer: process.env.FERROCHAT_URL
		? undefined
		: {
				command: `rm -rf /tmp/ferrochat-e2e && FERROCHAT_DATA_DIR=/tmp/ferrochat-e2e FERROCHAT_FRONTEND_DIR=./build FERROCHAT_PORT=${port} cargo run --manifest-path ../backend/Cargo.toml --bin ferrochat`,
				url: `http://127.0.0.1:${port}/health`,
				reuseExistingServer: false,
				timeout: 600000
			}
});
