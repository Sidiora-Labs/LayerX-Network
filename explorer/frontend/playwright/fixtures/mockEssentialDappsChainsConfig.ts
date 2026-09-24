import type { TestFixture, Page } from '@playwright/test';

import * as chainDataMock from 'mocks/multichain/chains';

export type MockEssentialDappsChainsConfigFixture = () => Promise<void>;

const fixture: TestFixture<MockEssentialDappsChainsConfigFixture, { page: Page }> = async({ page }, use) => {
  await use(async() => {
    await page.evaluate(([ chainDataMock ]) => {
      window.__essentialDappsChains = {
        chains: [
          chainDataMock.chainA,
        ],
      };
    }, [ chainDataMock ]);
  });
};

export default fixture;
