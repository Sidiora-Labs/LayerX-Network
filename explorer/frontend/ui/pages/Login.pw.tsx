import React from 'react';

import { test, expect } from 'playwright/lib';

import Login from './Login';

test.fixme('has feature text', async({ render, mockFeatures }) => {
  await mockFeatures([
    [ 'test_value', 'kitty' ],
  ]);
  const component = await render(<Login/>);
  const featureText = component.getByText('kitty');
  await expect(featureText).toBeVisible();
});
