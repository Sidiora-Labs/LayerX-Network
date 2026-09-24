import React from 'react';

import * as addressMock from 'mocks/address/address';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import AddressFromTo from './AddressFromTo';

test.use({ viewport: pwConfig.viewport.mobile });

test('outgoing txn', async({ render }) => {
  const component = await render(
    <AddressFromTo
      from={ addressMock.withoutName }
      to={{ ...addressMock.withName, hash: '0xa8FCe579a11E551635b9c9CB915BEcd873C51254' }}
      current={ addressMock.withoutName.hash }
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('incoming txn', async({ render }) => {
  const component = await render(
    <AddressFromTo
      from={{ ...addressMock.withName, hash: '0xa8FCe579a11E551635b9c9CB915BEcd873C51254' }}
      to={ addressMock.withoutName }
      current={ addressMock.withoutName.hash }
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('compact mode', async({ render }) => {
  const component = await render(
    <AddressFromTo
      from={ addressMock.withoutName }
      to={{ ...addressMock.withName, hash: '0xa8FCe579a11E551635b9c9CB915BEcd873C51254' }}
      mode="compact"
    />,
  );
  await expect(component).toHaveScreenshot();
});

test('loading state', async({ render }) => {
  const component = await render(
    <AddressFromTo
      from={ addressMock.withoutName }
      to={{ ...addressMock.withName, hash: '0xa8FCe579a11E551635b9c9CB915BEcd873C51254' }}
      isLoading
    />,
  );
  await expect(component).toHaveScreenshot();
});
