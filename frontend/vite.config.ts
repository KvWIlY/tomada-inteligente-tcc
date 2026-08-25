import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath, URL } from 'node:url';

// Este arquivo NAO existia ate a feature 007: o projeto rodava inteiramente no
// default do Vite. Ele passa a existir por causa do plugin do Tailwind 4.
//
// @tailwindcss/vite precisa ser >= 4.3 aqui: o 4.1.x declara peer
// `vite@^5.2.0 || ^6 || ^7` e este projeto esta no Vite 8.
export default defineConfig({
  plugins: [tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
});
