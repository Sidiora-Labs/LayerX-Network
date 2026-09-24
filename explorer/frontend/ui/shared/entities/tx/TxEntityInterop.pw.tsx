import React from 'react';

import * as interopMock from 'mocks/interop/interop';
import { test, expect } from 'playwright/lib';

import TxEntityInterop from './TxEntityInterop';

const hash = '0x376db52955d5bce114d0ccea2dcf22289b4eae1b86bcae5a59bb5fdbfef48899';

test.use({ viewport: { width: 180, height: 30 } });

test('with chain icon', async({ render, mockAssetResponse }) => {
  await mockAssetResponse('https://example.com/logo.png', './playwright/mocks/image_svg.svg');

  const component = await render(
    <TxEntityInterop
      hash={ hash }
      chain={ interopMock.chain }
    />,
  );

  await expect(component).toHaveScreenshot();
});

test('with chain icon stub +@dark-mode', async({ render }) => {
  const component = await render(
    <TxEntityInterop
      hash={ hash }
      chain={{ ...interopMock.chain, chain_logo: null }}
    />,
  );

  await expect(component).toHaveScreenshot();
});
