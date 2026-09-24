import tsconfigPaths from 'vite-tsconfig-paths';

import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [
    tsconfigPaths({
      ignoreConfigErrors: true,
    }),
  ],
  test: {
    globalSetup: [ './vitest/global-setup.ts' ],
    setupFiles: [ './vitest/setup.ts' ],
    include: [ '**/*.spec.ts', '**/*.spec.tsx' ],
    exclude: [ '**/node_modules/**', '**/node_modules_linux/**' ],
  },
});
