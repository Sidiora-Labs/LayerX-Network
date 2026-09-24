import React from 'react';

import * as addressMetadataMock from 'mocks/metadata/address';
import { test, expect } from 'playwright/lib';

import EntityTags from './EntityTags';

test('mixed name length +@mobile', async({ render }) => {
  const data = [
    addressMetadataMock.protocolTagWithMeta,
    addressMetadataMock.infoTagWithLink,
    addressMetadataMock.genericTag,
    addressMetadataMock.tagWithTooltip,
  ];
  const component = await render(<EntityTags tags={ data }/>);
  await expect(component).toHaveScreenshot();
});

test('one tag with long name +@mobile -@default', async({ render }) => {
  const data = [
    addressMetadataMock.infoTagWithLink,
  ];
  const component = await render(<EntityTags tags={ data }/>);
  await expect(component).toHaveScreenshot();
});
