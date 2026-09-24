import { noop } from 'es-toolkit';
import React from 'react';

import { test, expect } from 'playwright/lib';

import ContractSubmitAuditForm from './ContractSubmitAuditForm';

test('base view', async({ render }) => {
  const component = await render(<ContractSubmitAuditForm address="hash" onSuccess={ noop }/>);
  await expect(component).toHaveScreenshot();
});
