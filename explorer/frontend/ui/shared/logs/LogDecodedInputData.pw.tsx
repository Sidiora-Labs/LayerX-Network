import React from 'react';

import * as mocks from 'mocks/txs/decodedInputData';
import { test, expect } from 'playwright/lib';

import LogDecodedInputData from './LogDecodedInputData';

test('with indexed fields +@mobile +@dark-mode', async({ render }) => {
  const component = await render(<LogDecodedInputData data={ mocks.withIndexedFields }/>);
  await expect(component).toHaveScreenshot();
});

test('without indexed fields +@mobile', async({ render }) => {
  const component = await render(<LogDecodedInputData data={ mocks.withoutIndexedFields }/>);
  await expect(component).toHaveScreenshot();
});
