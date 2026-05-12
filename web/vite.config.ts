import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	server: {
		// Per ADR-0002: dev mode proxies /api/* to studio-server on :7878.
		// Release builds (M3) embed the static export via rust-embed; same-origin.
		proxy: {
			'/api': {
				target: 'http://127.0.0.1:7878',
				changeOrigin: true,
				ws: false
			}
		}
	}
});
