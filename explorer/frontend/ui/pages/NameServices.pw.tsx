import React from 'react';

import { clustersDirectoryMock } from 'mocks/clusters/directory';
import { clustersLeaderboardMock } from 'mocks/clusters/leaderboard';
import * as ensDomainMock from 'mocks/ens/domain';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect, devices } from 'playwright/lib';

import NameServices from './NameServices';

test.describe('domains', () => {

  const hooksConfig = {
    router: {
      query: { tab: 'domains' },
    },
  };

  test.beforeEach(async({ mockApiResponse, mockAssetResponse, mockTextAd, mockEnvs }) => {
    await mockTextAd();
    await mockEnvs([
      ...ENVS_MAP.clusters,
      ...ENVS_MAP.nameService,
    ]);
    await mockAssetResponse(ensDomainMock.protocolA.icon_url as string, './playwright/mocks/image_s.jpg');
    await mockAssetResponse(ensDomainMock.protocolB.icon_url as string, './playwright/mocks/image_md.jpg');
    await mockApiResponse('bens:domains_lookup', {
      items: [
        ensDomainMock.ensDomainA,
        ensDomainMock.ensDomainB,
        ensDomainMock.ensDomainC,
        ensDomainMock.ensDomainD,
      ],
      next_page_params: {
        page_token: '<token>',
        page_size: 50,
      },
    }, {
      queryParams: { only_active: true, protocols: [ ensDomainMock.protocolA.id, ensDomainMock.protocolB.id ] },
    });
    await mockApiResponse('bens:protocols', {
      items: [ ensDomainMock.protocolA, ensDomainMock.protocolB ],
    });
  });

  test('default view', async({ render }) => {
    test.slow();
    const component = await render(<NameServices/>, { hooksConfig });
    await expect(component).toHaveScreenshot({ timeout: 10_000 });
  });

  test.describe('mobile', () => {
    test.use({ viewport: devices['iPhone 13 Pro'].viewport });

    test('default view', async({ render }) => {
      const component = await render(<NameServices/>, { hooksConfig });
      await expect(component).toHaveScreenshot({ timeout: 10_000 });
    });
  });

  test('filters', async({ render, page }) => {
    const component = await render(<NameServices/>, { hooksConfig });

    await component.getByRole('button', { name: 'Filter' }).click();
    await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 250, height: 500 } });
  });
});

test.describe('directories', () => {

  const CDN_URL = 'https://localhost:3000/cdn';

  const hooksConfig = {
    directories: {
      router: {
        query: { tab: 'directories', view: 'directory' },
        isReady: true,
      },
    },
    leaderboard: {
      router: {
        query: { tab: 'directories', view: 'leaderboard' },
        isReady: true,
      },
    },
  };

  test.beforeEach(async({ mockEnvs, mockTextAd, mockApiResponse, mockAssetResponse }) => {
    await mockEnvs([
      ...ENVS_MAP.clusters,
      [ 'NEXT_PUBLIC_CLUSTERS_CDN_URL', CDN_URL ],
    ]);
    await mockTextAd();
    await mockAssetResponse(`${ CDN_URL }/profile-image/campnetwork/lol`, './playwright/mocks/image_s.jpg');
    await mockAssetResponse(`${ CDN_URL }/profile-image/duck/quack`, './playwright/mocks/image_s.jpg');
    await mockAssetResponse(`${ CDN_URL }/profile-image/test/cluster`, './playwright/mocks/image_s.jpg');
    await mockApiResponse('clusters:get_directory', clustersDirectoryMock, {
      queryParams: {
        input: JSON.stringify({
          offset: 0,
          limit: 50,
          orderBy: 'createdAt-desc',
          query: '',
        }),
      },
    });
    await mockApiResponse('clusters:get_leaderboard', clustersLeaderboardMock, {
      queryParams: {
        input: JSON.stringify({
          offset: 0,
          limit: 50,
          orderBy: 'rank-asc',
        }),
      },
    });
  });

  test('directory view', async({ render }) => {
    const component = await render(<NameServices/>, { hooksConfig: hooksConfig.directories });
    await expect(component).toHaveScreenshot();
  });

  test('leaderboard view', async({ render }) => {
    const component = await render(<NameServices/>, { hooksConfig: hooksConfig.leaderboard });
    await expect(component).toHaveScreenshot();
  });

  test.describe('mobile', () => {
    test.use({ viewport: devices['iPhone 13 Pro'].viewport });

    test('directory view', async({ render }) => {
      const component = await render(<NameServices/>, { hooksConfig: hooksConfig.directories });
      await expect(component).toHaveScreenshot();
    });

    test('leaderboard view', async({ render }) => {
      const component = await render(<NameServices/>, { hooksConfig: hooksConfig.leaderboard });
      await expect(component).toHaveScreenshot();
    });
  });
});
