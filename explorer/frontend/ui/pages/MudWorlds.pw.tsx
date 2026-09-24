import React from 'react';

import { mudWorlds } from 'mocks/mud/mudWorlds';
import { test, expect } from 'playwright/lib';

import MudWorlds from './MudWorlds';

test('default view +@mobile', async({ mockTextAd, mockApiResponse, render }) => {
  await mockTextAd();
  await mockApiResponse('general:mud_worlds', mudWorlds);
  const component = await render(<MudWorlds/>);
  await expect(component).toHaveScreenshot();
});
