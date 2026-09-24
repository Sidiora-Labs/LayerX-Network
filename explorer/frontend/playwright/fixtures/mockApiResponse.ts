import type { TestFixture, Page } from '@playwright/test';

import buildUrl from 'lib/api/buildUrl';
import type { ResourceName, ResourcePayload } from 'lib/api/resources';

interface Options<R extends ResourceName> {
  pathParams?: Parameters<typeof buildUrl<R>>[1];
  queryParams?: Parameters<typeof buildUrl<R>>[2];
  chainConfig?: Parameters<typeof buildUrl<R>>[4];
  times?: number;
  status?: number;
}

export type MockApiResponseFixture = <R extends ResourceName>(resourceName: R, responseMock: ResourcePayload<R>, options?: Options<R>) => Promise<string>;

const fixture: TestFixture<MockApiResponseFixture, { page: Page }> = async({ page }, use) => {
  await use(async(resourceName, responseMock, options) => {
    const apiUrl = buildUrl(resourceName, options?.pathParams, options?.queryParams, undefined, options?.chainConfig);

    await page.route(apiUrl, (route) => route.fulfill({
      status: options?.status ?? 200,
      json: responseMock,
    }), { times: options?.times });

    return apiUrl;
  });
};

export default fixture;
