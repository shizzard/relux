import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [
    svelte({
      // Inline styles into the JS bundle so the build produces a single .js
      // file — required for the binary-embed pipeline (commit 7).
      emitCss: false,
    }),
  ],
  build: {
    lib: {
      entry: 'src/main.ts',
      name: 'ReluxViewer',
      formats: ['iife'],
      fileName: () => 'relux-viewer.js',
    },
    outDir: 'dist',
    minify: 'esbuild',
    emptyOutDir: true,
    // Don't copy public/ (dev fixtures) into the production bundle dir.
    copyPublicDir: false,
  },
});
