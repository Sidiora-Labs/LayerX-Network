import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { AddressMetadataTagFormatted } from 'types/client/addressMetadata';

import * as actionButtonMetadataMock from 'mocks/metadata/appActionButton';
import { test, expect } from 'playwright/lib';

import AppActionButton from './AppActionButton';

test.beforeEach(async({ mockAssetResponse }) => {
  await mockAssetResponse(actionButtonMetadataMock.appLogoURL as string, './playwright/mocks/image_s.jpg');
});

test('button without styles +@dark-mode', async({ render }) => {
  const component = await render(
    <Flex w="200px">
      <AppActionButton
        data={ actionButtonMetadataMock.buttonWithoutStyles as NonNullable<AddressMetadataTagFormatted['meta']> }
        source="Txn"
      />
    </Flex>,
  );
  await expect(component).toHaveScreenshot();
});

test('link without styles +@dark-mode', async({ render }) => {
  const component = await render(
    <Flex w="200px">
      <AppActionButton
        data={ actionButtonMetadataMock.linkWithoutStyles as NonNullable<AddressMetadataTagFormatted['meta']> }
        source="Txn"
      />
    </Flex>,
  );
  await expect(component).toHaveScreenshot();
});

test('button with styles', async({ render }) => {
  const component = await render(
    <Flex w="200px">
      <AppActionButton
        data={ actionButtonMetadataMock.buttonWithStyles as NonNullable<AddressMetadataTagFormatted['meta']> }
        source="Txn"
      />
    </Flex>,
  );
  await expect(component).toHaveScreenshot();
});

test('link with styles', async({ render }) => {
  const component = await render(
    <Flex w="200px">
      <AppActionButton
        data={ actionButtonMetadataMock.linkWithStyles as NonNullable<AddressMetadataTagFormatted['meta']> }
        source="Txn"
      />
    </Flex>,
  );
  await expect(component).toHaveScreenshot();
});
