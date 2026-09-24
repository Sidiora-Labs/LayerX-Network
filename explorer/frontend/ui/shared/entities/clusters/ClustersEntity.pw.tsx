import React from 'react';

import { longNameCluster } from 'mocks/clusters/directory';
import { test, expect } from 'playwright/lib';

import ClustersEntity from './ClustersEntity';

test.use({ viewport: { width: 300, height: 200 } });

test.describe('basic display', () => {
  test('basic cluster entity', async({ render, mockAssetResponse }) => {
    await mockAssetResponse('https://cdn.clusters.xyz/profile-image/example.cluster', './playwright/mocks/image_s.jpg');
    const component = await render(
      <ClustersEntity
        clusterName="example.cluster"
      />,
    );

    await expect(component.getByText('example.cluster/')).toBeVisible();
    await expect(component).toHaveScreenshot();
  });

  test('cluster with subdomain', async({ render, mockAssetResponse }) => {
    await mockAssetResponse('https://cdn.clusters.xyz/profile-image/test/subdomain', './playwright/mocks/image_s.jpg');
    const component = await render(
      <ClustersEntity
        clusterName="test/subdomain"
      />,
    );

    await expect(component.getByText('test/subdomain')).toBeVisible();
    await expect(component).toHaveScreenshot();
  });
});

test.describe('variants', () => {
  test('heading variant', async({ render, mockAssetResponse }) => {
    await mockAssetResponse('https://cdn.clusters.xyz/profile-image/example.cluster', './playwright/mocks/image_s.jpg');
    const component = await render(
      <ClustersEntity
        clusterName="example.cluster"
        variant="heading"
      />,
    );

    await expect(component).toHaveScreenshot();
  });

  test('subheading variant', async({ render, mockAssetResponse }) => {
    await mockAssetResponse('https://cdn.clusters.xyz/profile-image/example.cluster', './playwright/mocks/image_s.jpg');
    const component = await render(
      <ClustersEntity
        clusterName="example.cluster"
        variant="subheading"
      />,
    );

    await expect(component).toHaveScreenshot();
  });
});

test.describe('customization', () => {
  test('no link', async({ render, mockAssetResponse }) => {
    await mockAssetResponse('https://cdn.clusters.xyz/profile-image/example.cluster', './playwright/mocks/image_s.jpg');
    const component = await render(
      <ClustersEntity
        clusterName="example.cluster"
        noLink={ true }
      />,
    );

    await expect(component.getByText('example.cluster/')).toBeVisible();
    await expect(component).toHaveScreenshot();
  });

  test('no icon', async({ render }) => {
    const component = await render(
      <ClustersEntity
        clusterName="example.cluster"
        noIcon={ true }
      />,
    );

    await expect(component.getByText('example.cluster/')).toBeVisible();
    await expect(component).toHaveScreenshot();
  });
});

test('long cluster name truncation', async({ render, mockAssetResponse }) => {
  await mockAssetResponse(
    'https://cdn.clusters.xyz/profile-image/this-is-a-very-long-cluster-name-that-should-test-truncation/subdomain', './playwright/mocks/image_s.jpg',
  );
  const component = await render(
    <ClustersEntity
      clusterName={ longNameCluster.name }
      href={ `/cluster/${ longNameCluster.name }` }
    />,
  );

  await expect(component.getByText(/this-is-a-very-long/)).toBeVisible();
  await expect(component).toHaveScreenshot();
});

test('hover interaction', async({ render, mockAssetResponse }) => {
  await mockAssetResponse('https://cdn.clusters.xyz/profile-image/example.cluster', './playwright/mocks/image_s.jpg');
  const component = await render(
    <ClustersEntity
      clusterName="example.cluster"
    />,
  );

  await component.hover();
  await expect(component).toHaveScreenshot();
});

test('dark mode +@dark-mode', async({ render, mockAssetResponse }) => {
  await mockAssetResponse('https://cdn.clusters.xyz/profile-image/example.cluster', './playwright/mocks/image_s.jpg');
  const component = await render(
    <ClustersEntity
      clusterName="example.cluster"
    />,
  );

  await expect(component).toHaveScreenshot();
});
