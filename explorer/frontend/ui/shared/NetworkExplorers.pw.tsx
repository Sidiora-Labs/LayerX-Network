import React from 'react';

import { test, expect } from 'playwright/lib';

import NetworkExplorers from './NetworkExplorers';

test('base view', async({ render, page }) => {
  const component = await render(<NetworkExplorers type="tx" pathParam="0x123"/>);
  await component.getByLabel('Verify with other explorers').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 300, height: 150 } });
});
