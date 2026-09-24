import type { TestFixture, Page } from '@playwright/test';

export type MockAssetResponseFixture = (url: string, path: string) => Promise<void>;

const fixture: TestFixture<MockAssetResponseFixture, { page: Page }> = async({ page }, use) => {
  await use(async(url, path) => {

    await page.route(url, (route) => route.fulfill({
      status: 200,
      path,
    }));
  });
};

export default fixture;
