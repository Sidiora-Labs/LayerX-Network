import type { PlaywrightTestConfig } from '@playwright/experimental-ct-react';
import { devices, defineConfig } from '@playwright/experimental-ct-react';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'esbuild';
import svgr from 'vite-plugin-svgr';
import tsconfigPaths from 'vite-tsconfig-paths';

import appConfig from 'configs/app';

/**
 * See https://playwright.dev/docs/test-configuration.
 */
const config: PlaywrightTestConfig = defineConfig({
  testDir: './',

  testMatch: /.*\.pw\.tsx/,

  snapshotPathTemplate: '{testDir}/{testFileDir}/__screenshots__/{testFileName}_{projectName}_{arg}{ext}',

  /* Maximum time one test can run for. */
  timeout: 10 * 1000,

  /* Run tests in files in parallel */
  fullyParallel: true,

  /* Fail the build on CI if you accidentally left test.only in the source code. */
  forbidOnly: Boolean(process.env.CI),

  /* Retry on CI only */
  retries: process.env.CI ? 2 : 0,

  /* Opt out of parallel tests. */
  // on non-performant local machines some tests may fail due to lack of resources
  // so we opt out of parallel tests in any environment
  workers: 1,

  /* Reporter to use. See https://playwright.dev/docs/test-reporters */
  reporter: process.env.CI ? 'blob' : 'html',

  expect: {
    toHaveScreenshot: {
      threshold: 0.05,
    },
  },

  /* Shared settings for all the projects below. See https://playwright.dev/docs/api/class-testoptions. */
  use: {
    baseURL: appConfig.app.baseUrl,

    /* Collect trace when retrying the failed test. See https://playwright.dev/docs/trace-viewer */
    trace: 'on-first-retry',

    /* Port to use for Playwright component endpoint. */
    ctPort: 3100,

    headless: true,

    ctViteConfig: {
      plugins: [
        tsconfigPaths({ loose: true, ignoreConfigErrors: true }),
        react(),
        svgr({
          exportAsDefault: true,
        }),
      ] as unknown as Array<Plugin>,
      build: {
        // it actually frees some memory that vite needs a lot
        // https://github.com/storybookjs/builder-vite/issues/409#issuecomment-1152848986
        sourcemap: false,
        minify: false,
        rollupOptions: {
          output: {
            // Ensure __envs exists in every chunk so code (e.g. next/router, configs/app) that
            // runs before index.ts/envs.js doesn't throw. Real envs are set when envs.js runs.
            //
            // Explanation:
            // With await import(...) in useAccount/useWallet, Vite turns those into separate chunks.
            // In Playwright CT, the component’s chunk (and its dependency graph, including useAccount → config → getEnvValue → __envs)
            // can load and run before the main entry that runs envs.js.
            // So when that chunk runs, window.__envs isn’t set yet and you get ReferenceError: __envs is not defined.
            //
            // https://rollupjs.org/configuration-options/#output-banner-output-footer
            banner: '(function(){if(typeof window!==\'undefined\')window.__envs=window.__envs||{};})();',
          },
        },
      },
      resolve: {
        alias: [
          // There is an issue with building these package using vite that I cannot resolve
          // The solution described here - https://github.com/vitejs/vite/issues/9703#issuecomment-1216662109
          // doesn't seam to work well with our setup
          // so for now we just mock these modules in tests
          { find: '@metamask/post-message-stream', replacement: './playwright/mocks/modules/@metamask/post-message-stream.js' },
          { find: '@metamask/providers', replacement: './playwright/mocks/modules/@metamask/providers.js' },

          // '@metamask/sdk imports the browser module as UMD, but @wagmi/connectors expects it to be ESM
          // so we do a little remapping here
          { find: '@metamask/sdk', replacement: './node_modules/@metamask/sdk/dist/browser/es/metamask-sdk.js' },

          // Mock for growthbook to test feature flags
          { find: 'lib/growthbook/useFeatureValue', replacement: './playwright/mocks/lib/growthbook/useFeatureValue.js' },

          // Mock for reCaptcha hook
          { find: 'ui/shared/reCaptcha/useReCaptcha', replacement: './playwright/mocks/ui/shared/recaptcha/useReCaptcha.js' },

          // The createWeb3Modal() function from web3modal/wagmi/react somehow pollutes the global styles which causes the tests to fail
          // We don't call this function in TestApp and since we use useWeb3Modal() and useWeb3ModalState() hooks in the code, we have to mock the module
          // Otherwise it will complain that createWeb3Modal() is no called before the hooks are used
          { find: /^@reown\/appkit\/react$/, replacement: './playwright/mocks/modules/@reown/appkit/react.js' },

          { find: '/playwright/index.ts', replacement: './playwright/index.ts' },
          { find: '/playwright/envs.js', replacement: './playwright/envs.js' },

          // Fix for @libp2p/utils missing merge-options export
          { find: '@libp2p/utils/merge-options', replacement: 'merge-options' },

          // Mock for @helia/verified-fetch to avoid build issues in tests
          { find: '@helia/verified-fetch', replacement: './playwright/mocks/modules/@helia/verified-fetch.js' },

          // Mock for @specify-sh/sdk to avoid build issues in tests
          { find: '@specify-sh/sdk', replacement: './playwright/mocks/modules/@specify-sh/sdk.js' },
        ],
      },
      define: {
        'process.env': '__envs', // Port process.env over window.__envs
      },
    },
  },

  // configured projects
  // these projects are also used for sharding tests in CI
  // when adding or deleting a project, make sure to update github workflow accordingly
  projects: [
    {
      name: 'default',
      grepInvert: /-@default/,
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1200, height: 750 },
      },
    },
    {
      name: 'mobile',
      grep: /\+@mobile/,
      use: {
        ...devices['iPhone 13 Pro'],
      },
    },
    {
      name: 'dark-color-mode',
      grep: /\+@dark-mode/,
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1200, height: 750 },
        colorScheme: 'dark',
      },
    },
  ],
});

export default config;
