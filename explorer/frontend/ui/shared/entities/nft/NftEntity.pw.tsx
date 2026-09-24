import React from 'react';

import { test, expect } from 'playwright/lib';

import NftEntity from './NftEntity';

const variants = [ 'subheading', 'content' ] as const;
const hash = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';

test.use({ viewport: { width: 180, height: 30 } });

test.describe('variants', () => {
  variants.forEach((variant) => {
    test(`${ variant }`, async({ render }) => {
      const component = await render(
        <NftEntity
          hash={ hash }
          id="1042"
          variant={ variant }
        />,
      );

      await expect(component).toHaveScreenshot();
    });
  });
});

test('loading', async({ render }) => {
  const component = await render(
    <NftEntity
      hash={ hash }
      id="1042"
      isLoading
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('long id', async({ render }) => {
  const component = await render(
    <NftEntity
      hash={ hash }
      id="1794350723452223"
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('customization', async({ render }) => {
  const component = await render(
    <NftEntity
      hash={ hash }
      id="1042"
      p={ 3 }
      borderWidth="1px"
      borderColor="blue.700"
    />,
  );

  await expect(component).toHaveScreenshot();
});
