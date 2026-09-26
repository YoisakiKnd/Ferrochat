import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// /** @type {import('vite').Plugin} */
// const viteServerConfig = {
// 	name: 'log-request-middleware',
// 	configureServer(server) {
// 		server.middlewares.use((req, res, next) => {
// 			res.setHeader('Access-Control-Allow-Origin', '*');
// 			res.setHeader('Access-Control-Allow-Methods', 'GET');
// 			res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
// 			res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
// 			next();
// 		});
// 	}
// };

export default defineConfig({
	plugins: [sveltekit()],
	// Strip leftover upstream `console.log/info/debug` from the production bundle
	// (200+ call sites in Open WebUI components; some leak user content).
	// warn/error stay. Template-expression logs need DEV guards (see MarkdownTokens.svelte).
	esbuild: {
		pure: ['console.log', 'console.info', 'console.debug']
	},
	define: {
		APP_VERSION: JSON.stringify(process.env.npm_package_version),
		APP_BUILD_HASH: JSON.stringify(process.env.APP_BUILD_HASH || 'dev-build')
	},
	build: {
		sourcemap: false
	},
	worker: {
		format: 'es'
	}
});
