// @ts-check
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'astro/config';

export default defineConfig({
  server: { port: 1420, host: false },
  vite: {
    plugins: [tailwindcss()],
    clearScreen: false,
    server: { strictPort: true },
  },
});
