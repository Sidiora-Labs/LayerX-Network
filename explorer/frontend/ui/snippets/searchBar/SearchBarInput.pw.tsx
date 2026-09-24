import React from 'react';

import { test, expect } from 'playwright/lib';

import SearchBarInput from './SearchBarInput';

const props = {
  onChange: () => {},
  onSubmit: () => {},
  onClear: () => {},
  value: 'duck duck',
};

test('input on regular page +@mobile +@dark-mode', async({ render, page }) => {
  await render(<SearchBarInput { ...props }/>);
  const input = page.getByPlaceholder(/search by/i);
  await expect(input).toHaveScreenshot();
});

test('input on home page +@mobile +@dark-mode', async({ render, page }) => {
  await render(<SearchBarInput { ...props } isHeroBanner/>);
  const input = page.getByPlaceholder(/search by/i);
  await expect(input).toHaveScreenshot();
});
