import React from 'react';

import { FOOTER_LINKS } from 'mocks/config/footerLinks';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import Footer from './Footer';

const FOOTER_LINKS_URL = 'https://localhost:3000/footer-links.json';

test.describe('with custom links, max cols', () => {
  test.beforeEach(async({ render, mockApiResponse, mockConfigResponse, injectMetaMaskProvider, mockEnvs, mockAssetResponse }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_FOOTER_LINKS', FOOTER_LINKS_URL ],
    ]);
    await mockConfigResponse('NEXT_PUBLIC_FOOTER_LINKS', FOOTER_LINKS_URL, FOOTER_LINKS);
    await injectMetaMaskProvider();
    await mockApiResponse('general:homepage_indexing_status', {
      finished_indexing: false,
      finished_indexing_blocks: false,
      indexed_internal_transactions_ratio: '0.1',
      indexed_blocks_ratio: '0.1',
    });
    await mockAssetResponse(FOOTER_LINKS[3].links[0].iconUrl?.[0]!, './playwright/mocks/image_s.jpg');
    await mockAssetResponse(FOOTER_LINKS[3].links[0].iconUrl?.[1]!, './playwright/mocks/image_svg.svg');

    await render(<Footer/>);
  });

  test('+@mobile +@dark-mode', async({ page }) => {
    await expect(page).toHaveScreenshot();
  });

  test.describe('screen xl', () => {
    test.use({ viewport: pwConfig.viewport.xl });

    test('base view', async({ page }) => {
      await expect(page).toHaveScreenshot();
    });
  });
});

test.describe('with custom links, min cols', () => {
  test('base view +@dark-mode +@mobile', async({ render, page, mockConfigResponse, mockEnvs }) => {
    await mockEnvs([
      [ 'NEXT_PUBLIC_FOOTER_LINKS', FOOTER_LINKS_URL ],
    ]);
    await mockConfigResponse('NEXT_PUBLIC_FOOTER_LINKS', FOOTER_LINKS_URL, [ FOOTER_LINKS[0] ]);
    await render(<Footer/>);
    await expect(page).toHaveScreenshot();
  });
});

test.describe('without custom links', () => {
  test('base view +@dark-mode +@mobile', async({ render, page, injectMetaMaskProvider, mockApiResponse }) => {
    await injectMetaMaskProvider();
    await mockApiResponse('general:config_backend_version', { backend_version: 'v5.2.0-beta.+commit.1ce1a355' });
    await render(<Footer/>);
    await expect(page).toHaveScreenshot();
  });

  test('with indexing alert +@dark-mode +@mobile', async({ render, injectMetaMaskProvider, mockApiResponse }) => {
    await injectMetaMaskProvider();
    await mockApiResponse('general:homepage_indexing_status', {
      finished_indexing: false,
      finished_indexing_blocks: false,
      indexed_internal_transactions_ratio: '0.1',
      indexed_blocks_ratio: '0.1',
    });

    const component = await render(<Footer/>);
    await expect(component).toHaveScreenshot();
  });
});
