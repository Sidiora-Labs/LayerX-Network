import type { TestFixture, Page } from '@playwright/test';
import { isEqual } from 'es-toolkit';
import type { PublicRpcSchema } from 'viem';

import { getEnvValue } from 'configs/app/utils';

type Params = Array<PublicRpcSchema[number] & { status?: number }>;

export type MockRpcResponseFixture = (params: Params) => Promise<void>;

const fixture: TestFixture<MockRpcResponseFixture, { page: Page }> = async({ page }, use) => {
  await use(async(rpcMocks) => {
    const rpcUrl = getEnvValue('NEXT_PUBLIC_NETWORK_RPC_URL');

    if (!rpcUrl) {
      return;
    }

    await page.route(rpcUrl, (route) => {
      const method = route.request().method();

      if (method !== 'POST') {
        route.continue();
        return;
      }

      const json = route.request().postDataJSON();
      const id = json?.id;

      if (id === undefined) {
        route.continue();
        return;
      }

      const rpcMock = rpcMocks.find((mock) => {
        const payload = {
          id,
          jsonrpc: '2.0',
          method: mock.Method,
          ...(mock.Parameters ? { params: mock.Parameters } : {}),
        };

        return isEqual(json, payload);
      });

      if (rpcMock) {
        if (rpcMock.status && rpcMock.status !== 200) {
          return route.fulfill({
            status: rpcMock.status,
            json: {
              id,
              jsonrpc: '2.0',
              error: {
                code: -32000,
                message: 'Internal error',
              },
            },
          });
        }

        return route.fulfill({
          status: 200,
          json: {
            id,
            jsonrpc: '2.0',
            result: rpcMock.ReturnType,
          },
        });
      }
    });
  });
};

export default fixture;
