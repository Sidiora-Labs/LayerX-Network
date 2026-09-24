import type { TestFixture, Page } from '@playwright/test';

export type MockFeaturesFixture = (features: Array<[string, unknown]>) => Promise<void>;

const fixture: TestFixture<MockFeaturesFixture, { page: Page }> = async({ page }, use) => {
  await use(async(features) => {
    for (const [ name, value ] of features) {
      await page.evaluate(({ name, value }) => {
        window.localStorage.setItem(`pw_feature:${ name }`, JSON.stringify(value));
      }, { name, value });
    }
  });
};

export default fixture;
