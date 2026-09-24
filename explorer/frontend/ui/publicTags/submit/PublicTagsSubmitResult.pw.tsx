import React from 'react';

import { expect, test, devices } from 'playwright/lib';

import * as mocks from './mocks';
import PublicTagsSubmitResult from './PublicTagsSubmitResult';

test('all success result view +@mobile', async({ render }) => {
  const component = await render(<PublicTagsSubmitResult data={ mocks.allSuccessResponses }/>);
  await expect(component).toHaveScreenshot();
});

test('result with errors view', async({ render }) => {
  const component = await render(<PublicTagsSubmitResult data={ mocks.mixedResponses }/>);
  await expect(component).toHaveScreenshot();
});

test.describe('mobile', () => {
  test.use({ viewport: devices['iPhone 13 Pro'].viewport });

  test('result with errors view', async({ render }) => {
    const component = await render(<PublicTagsSubmitResult data={ mocks.mixedResponses }/>);
    await expect(component).toHaveScreenshot();
  });
});
