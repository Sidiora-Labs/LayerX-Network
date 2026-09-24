import react from '@vitejs/plugin-react';
import { resolve } from 'path';
import { defineConfig } from 'vite';
import dts from 'vite-plugin-dts';
import svgr from 'vite-plugin-svgr';
import tsconfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  plugins: [
    react(),
    tsconfigPaths(),
    svgr({
      include: '**/*.svg',
      exclude: '',
      svgrOptions: {
        icon: true,
        svgo: true,
        plugins: [ '@svgr/plugin-jsx' ],
        svgoConfig: {
          plugins: [
            {
              name: 'preset-default',
              params: {
                overrides: {
                  removeViewBox: false,
                  removeHiddenElems: false,
                },
              },
            },
            'removeDimensions',
          ],
        },
      },
    }),
    dts({
      include: [
        '../chakra/**/*.tsx',
        '../theme/**/*.ts',
        '../components/**/*',
        '../utils/**/*.ts',
        '../hooks/**/*.tsx',
        './src/**/*.ts',
        '../../global.d.ts',
        '../../decs.d.ts',
        '../../reset.d.ts',
      ],
      exclude: [
        '../**/*.pw.tsx',
        '../**/*.pw.ts',
        '../**/*.test.*',
        '../**/*.spec.*',
      ],
      rollupTypes: false,
      outDir: './dist',
      insertTypesEntry: true,
      entryRoot: '..',
    }),
  ],
  resolve: {
    alias: {
      configs: resolve(__dirname, '../../configs'),
      lib: resolve(__dirname, '../../lib'),
      types: resolve(__dirname, '../../types'),
      icons: resolve(__dirname, '../../icons'),
      ui: resolve(__dirname, '../../ui'),
      'public': resolve(__dirname, '../../public'),
      toolkit: resolve(__dirname, '../'),
    },
  },
  build: {
    lib: {
      entry: resolve(__dirname, './src/index.ts'),
      formats: [ 'es' ],
      fileName: 'index',
    },
    outDir: './dist',
    rollupOptions: {
      external: [
        'react',
        'react-dom',
        'react/jsx-runtime',
        'react/jsx-dev-runtime',
        '@chakra-ui/react',
        '@emotion/react',
        'next/link',
        'next/router',
        'next-themes',
        'react-hook-form',
        'es-toolkit',
        'dayjs',
        'd3',
        'dom-to-image',
        '@uidotdev/usehooks',
      ],
      output: {
        preserveModules: false,
        preserveModulesRoot: '..',
        exports: 'named',
        interop: 'auto',
      },
    },
  },
});
