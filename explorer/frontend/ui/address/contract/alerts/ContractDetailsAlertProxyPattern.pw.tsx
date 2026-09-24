import React from 'react';

import { test, expect } from 'playwright/lib';

import ContractDetailsAlertProxyPattern from './ContractDetailsAlertProxyPattern';

test('proxy type with link +@mobile', async({ render }) => {
  const component = await render(<ContractDetailsAlertProxyPattern type="eip1167" isLoading={ false }/>);
  await expect(component).toHaveScreenshot();
});

test('proxy type with link but without description', async({ render }) => {
  const component = await render(<ContractDetailsAlertProxyPattern type="master_copy" isLoading={ false }/>);
  await expect(component).toHaveScreenshot();
});

test('proxy type without link', async({ render }) => {
  const component = await render(<ContractDetailsAlertProxyPattern type="basic_implementation" isLoading={ false }/>);
  await expect(component).toHaveScreenshot();
});

test('proxy type with conflicting implementations', async({ render, page }) => {
  const component = await render(
    <ContractDetailsAlertProxyPattern
      type="resolved_delegate_proxy"
      isLoading={ false }
      conflictingImplementations={ [
        {
          proxy_type: 'eip1967',
          implementations: [
            { address_hash: '0x568AA6C21cCf558C47F2A01B60cc6D549cED2F59' },
          ],
        },
        {
          proxy_type: 'eip1967_oz',
          implementations: [
            { address_hash: '0x7d608aBCf9a3BE6B869E745E6F8dB3434877D60F', name: 'Implementation 3' },
            { address_hash: '0x568AA6C21cCf558C47F2A01B60cc6D549cED2F59' },
          ],
        },
      ] }
    />,
  );
  await expect(component).toHaveScreenshot();

  page.getByText('View details').click();
  await expect(page).toHaveScreenshot();
});
