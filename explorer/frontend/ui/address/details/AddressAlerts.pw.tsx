import React from 'react';

import * as metadataMock from 'mocks/metadata/address';
import { test, expect } from 'playwright/lib';

import AddressAlerts from './AddressAlerts';

test('base view', async({ render }) => {
  const component = await render(<AddressAlerts tags={ [ metadataMock.noteTag, metadataMock.noteTag2 ] }/>);

  await expect(component).toHaveScreenshot();
});

test('with scam token', async({ render }) => {
  const component = await render(<AddressAlerts tags={ [ metadataMock.noteTag, metadataMock.noteTag2 ] } isScamToken={ true }/>);

  await expect(component).toHaveScreenshot();
});
