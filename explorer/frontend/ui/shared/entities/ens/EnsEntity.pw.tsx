import React from 'react';

import * as domainMock from 'mocks/ens/domain';
import { stableHover } from 'playwright/helpers/stableHover';
import { test, expect } from 'playwright/lib';

import EnsEntity from './EnsEntity';

const name = 'cat.eth';
const variants = [ 'subheading', 'content' ] as const;

test.use({ viewport: { width: 180, height: 30 } });

test.describe('variant', () => {
  variants.forEach((variant) => {
    test(`${ variant }`, async({ render }) => {
      const component = await render(
        <EnsEntity
          domain={ name }
          variant={ variant }
        />,
      );

      await expect(component).toHaveScreenshot();
    });
  });
});

test('loading', async({ render }) => {
  const component = await render(
    <EnsEntity
      domain={ name }
      isLoading
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('with long name', async({ render }) => {
  const component = await render(
    <EnsEntity
      domain="kitty.kitty.kitty.cat.eth"
    />,
  );

  await stableHover(component.getByText(name.slice(0, 4)));

  await expect(component).toHaveScreenshot();
});

test('customization', async({ render }) => {
  const component = await render(
    <EnsEntity
      domain={ name }
      p={ 3 }
      borderWidth="1px"
      borderColor="blue.700"
    />,
  );

  await expect(component).toHaveScreenshot();
});

test.describe('tooltip test', () => {
  test.use({ viewport: { width: 300, height: 400 } });

  test('with protocol info', async({ render, page, mockAssetResponse }) => {
    await mockAssetResponse(domainMock.ensDomainA.protocol?.icon_url as string, './playwright/mocks/image_s.jpg');

    const component = await render(
      <EnsEntity
        domain={ name }
        protocol={ domainMock.protocolA }
      />,
    );

    await component.getByAltText(`${ domainMock.protocolA.title } protocol icon`).first().hover();

    await expect(page.getByText(domainMock.protocolA.description)).toBeVisible();
    await expect(page).toHaveScreenshot();
  });
});
