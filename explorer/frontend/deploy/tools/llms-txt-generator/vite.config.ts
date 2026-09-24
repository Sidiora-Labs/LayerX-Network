import { resolve } from 'path';
import { defineConfig } from 'vite';

export default defineConfig({
  build: {
    lib: {
      entry: {
        index: resolve(__dirname, 'index.ts'),
      },
      formats: [ 'es' ],
      fileName: (format, entryName) => `${ entryName }.js`,
    },
    rollupOptions: {
      external: [ 'node:worker_threads', 'node:url', 'node:path', 'node:fs' ],
      output: {
        dir: 'dist',
        entryFileNames: '[name].js',
        chunkFileNames: '[name].js',
        assetFileNames: '[name].[ext]',
      },
    },
  },
  resolve: {
    alias: {
      configs: resolve(__dirname, '../../../configs'),
      lib: resolve(__dirname, '../../../lib'),
      toolkit: resolve(__dirname, '../../../toolkit'),
      types: resolve(__dirname, '../../../types'),
    },
    preserveSymlinks: true,
  },
});
