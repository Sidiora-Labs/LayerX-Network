import React from 'react';

import { test, expect } from 'playwright/lib';

import StatusTag from './StatusTag';

test('ok status', async({ page, render }) => {
  await render(<StatusTag type="ok" text="Test"/>);
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 75, height: 30 } });
});

test('error status', async({ page, render }) => {
  await render(<StatusTag type="error" text="Test"/>);
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 75, height: 30 } });
});

test('pending status', async({ page, render }) => {
  await render(<StatusTag type="pending" text="Test"/>);
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 75, height: 30 } });
});
