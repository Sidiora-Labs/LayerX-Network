/* eslint-disable no-console */
import { test as base } from '@playwright/experimental-ct-react';
import type { Page } from '@playwright/test';

import * as injectMetaMaskProvider from './fixtures/injectMetaMaskProvider';
import * as mockApiResponse from './fixtures/mockApiResponse';
import * as mockAssetResponse from './fixtures/mockAssetResponse';
import * as mockConfigResponse from './fixtures/mockConfigResponse';
import * as mockContractReadResponse from './fixtures/mockContractReadResponse';
import * as mockEnvs from './fixtures/mockEnvs';
import * as mockEssentialDappsChainsConfig from './fixtures/mockEssentialDappsChainsConfig';
import * as mockFeatures from './fixtures/mockFeatures';
import * as mockMultichainConfig from './fixtures/mockMultichainConfig';
import * as mockRpcResponse from './fixtures/mockRpcResponse';
import * as mockTextAd from './fixtures/mockTextAd';
import * as render from './fixtures/render';
import * as socketServer from './fixtures/socketServer';

export interface Fixtures {
  render: render.RenderFixture;
  mockApiResponse: mockApiResponse.MockApiResponseFixture;
  mockAssetResponse: mockAssetResponse.MockAssetResponseFixture;
  mockConfigResponse: mockConfigResponse.MockConfigResponseFixture;
  mockContractReadResponse: mockContractReadResponse.MockContractReadResponseFixture;
  mockEnvs: mockEnvs.MockEnvsFixture;
  mockFeatures: mockFeatures.MockFeaturesFixture;
  mockMultichainConfig: mockMultichainConfig.MockMultichainConfigFixture;
  mockEssentialDappsChainsConfig: mockEssentialDappsChainsConfig.MockEssentialDappsChainsConfigFixture;
  mockRpcResponse: mockRpcResponse.MockRpcResponseFixture;
  createSocket: socketServer.CreateSocketFixture;
  injectMetaMaskProvider: injectMetaMaskProvider.InjectMetaMaskProvider;
  mockTextAd: mockTextAd.MockTextAdFixture;
}

export type TestFnArgs = Fixtures & { page: Page };

const test = base.extend<Fixtures>({
  render: render.default,
  mockApiResponse: mockApiResponse.default,
  mockAssetResponse: mockAssetResponse.default,
  mockConfigResponse: mockConfigResponse.default,
  mockContractReadResponse: mockContractReadResponse.default,
  mockEnvs: mockEnvs.default,
  mockFeatures: mockFeatures.default,
  mockMultichainConfig: mockMultichainConfig.default,
  mockEssentialDappsChainsConfig: mockEssentialDappsChainsConfig.default,
  mockRpcResponse: mockRpcResponse.default,
  // FIXME: for some reason Playwright does not intercept requests to text ad provider when running multiple tests in parallel
  // even if we have a global request interceptor (maybe it is related to service worker issue, maybe not)
  // so we have to inject mockTextAd fixture in each test and mock the response where it is needed
  mockTextAd: mockTextAd.default,
  createSocket: socketServer.createSocket,
  injectMetaMaskProvider: injectMetaMaskProvider.default,
});

test.beforeEach(async({ page, mockTextAd }) => {
  // debug
  const isDebug = process.env.PWDEBUG === '1';

  if (isDebug) {
    page.on('console', msg => console.log(msg.text()));
    page.on('request', request => console.info('\x1b[34m%s\x1b[0m', '>>', request.method(), request.url()));
    page.on('response', response => console.info('\x1b[35m%s\x1b[0m', '<<', String(response.status()), response.url()));
  }

  // Abort all other requests to external resources
  await page.route('**', (route) => {
    if (!route.request().url().startsWith('http://localhost')) {
      isDebug && console.info('Aborting request to', route.request().url());
      route.abort();
    } else {
      route.continue();
    }
  });

  // with few exceptions:
  //  1. mock text AD requests
  await mockTextAd();
});

export * from '@playwright/experimental-ct-react';
export { test };
