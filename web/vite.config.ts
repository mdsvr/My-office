import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // The Rust server (step 3) answers these; the dev server forwards them.
  server: { proxy: { '/api': 'http://127.0.0.1:47400', '/events': 'http://127.0.0.1:47400' } },
});
