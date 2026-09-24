import { noop } from 'es-toolkit';
import React from 'react';

import { test, expect } from 'playwright/lib';

import TagGroupSelect from './TagGroupSelect';

test.use({ viewport: { width: 480, height: 140 } });

test('base view +@dark-mode', async({ render }) => {
  const component = await render(
    <TagGroupSelect
      items={ [ { id: '1', title: 'Option 1' }, { id: '2', title: 'Option 2' }, { id: 'duck', title: 'Cute little duck' } ] }
      value="duck"
      onChange={ noop }
    />,
  );

  await component.getByText('Option 2').hover();

  await expect(component).toHaveScreenshot();
});
