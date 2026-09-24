import type { TestFixture, Page } from '@playwright/test';

import * as chainDataMock from 'mocks/multichain/chains';

export type MockMultichainConfigFixture = () => Promise<void>;

const fixture: TestFixture<MockMultichainConfigFixture, { page: Page }> = async({ page }, use) => {
  await use(async() => {
    await page.evaluate(([ chainDataMock ]) => {
      window.__multichainConfig = {
        chains: [
          chainDataMock.chainA,
          chainDataMock.chainB,
          chainDataMock.chainC,
          chainDataMock.chainD,
          chainDataMock.chainE,
        ],
      };
    }, [ chainDataMock ]);
  });
};

export default fixture;
