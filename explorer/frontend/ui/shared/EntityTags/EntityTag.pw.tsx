import { Box } from '@chakra-ui/react';
import React from 'react';

import * as addressMetadataMock from 'mocks/metadata/address';
import { test, expect } from 'playwright/lib';

import EntityTag from './EntityTag';

test.use({ viewport: { width: 400, height: 300 } });

test('custom name tag +@dark-mode', async({ render }) => {
  const component = await render(<Box w="200px"><EntityTag data={ addressMetadataMock.customNameTag }/></Box>);
  await expect(component).toHaveScreenshot();
});

test('cex deposit tag', async({ render }) => {
  const component = await render(
    <Box w="200px">
      <EntityTag data={ addressMetadataMock.cexDepositTag } addressHash="0x1234567890123456789012345678901234567890"/>
    </Box>,
  );
  await expect(component).toHaveScreenshot();
});

test('warpcast tag', async({ render }) => {
  const component = await render(<Box w="200px"><EntityTag data={ addressMetadataMock.warpcastTag }/></Box>);
  await expect(component).toHaveScreenshot();
});

test('generic tag +@dark-mode', async({ render }) => {
  const component = await render(<Box w="200px"><EntityTag data={ addressMetadataMock.genericTag }/></Box>);
  await expect(component).toHaveScreenshot();
});

test('protocol tag +@dark-mode', async({ render }) => {
  const component = await render(<Box w="200px"><EntityTag data={ addressMetadataMock.protocolTag }/></Box>);
  await expect(component).toHaveScreenshot();
});

test('tag with link and long name +@dark-mode', async({ render }) => {
  const component = await render(<EntityTag data={ addressMetadataMock.infoTagWithLink } maxW="300px"/>);
  await expect(component).toHaveScreenshot();
});

test('tag with tooltip +@dark-mode', async({ render, page, mockAssetResponse }) => {
  await mockAssetResponse(addressMetadataMock.tagWithTooltip.meta?.tooltipIcon as string, './playwright/mocks/image_s.jpg');
  await mockAssetResponse(addressMetadataMock.tagWithTooltip.meta?.tooltipAttributionIcon as string, './playwright/mocks/image_md.jpg');
  const component = await render(<EntityTag data={ addressMetadataMock.tagWithTooltip }/>);
  await component.getByText('BlockscoutHeroes').hover();
  await page.getByText('Blockscout team member').waitFor({ state: 'visible' });
  await expect(page).toHaveScreenshot();
});
