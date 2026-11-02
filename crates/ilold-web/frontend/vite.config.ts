import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// changeOrigin sends the backend's own Host; the backend accepts this page's Origin only with ILOLD_DEV_ORIGIN set.
export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	server: {
		proxy: {
			'/api': { target: 'http://localhost:8080', changeOrigin: true },
			'/ws': {
				target: 'http://localhost:8080',
				ws: true,
				changeOrigin: true
			}
		}
	}
});
