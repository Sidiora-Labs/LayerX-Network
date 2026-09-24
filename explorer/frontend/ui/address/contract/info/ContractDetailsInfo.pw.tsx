import React from 'react';

import * as addressMock from 'mocks/address/address';
import { contractAudits } from 'mocks/contract/audits';
import * as contractMock from 'mocks/contract/info';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ContractDetailsInfo from './ContractDetailsInfo';

test('with certified icon', async({ render }) => {
  const props = {
    data: contractMock.certified,
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsInfo { ...props }/>);

  await expect(component).toHaveScreenshot();
});

test('zkSync contract', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.zkSyncRollup);
  const props = {
    data: contractMock.zkSync,
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsInfo { ...props }/>);

  await expect(component).toHaveScreenshot();
});

test('stylus rust contract', async({ render, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.zkSyncRollup);
  const props = {
    data: contractMock.stylusRust,
    isLoading: false,
    addressData: addressMock.contract,
  };
  const component = await render(<ContractDetailsInfo { ...props }/>);

  await expect(component).toHaveScreenshot();
});

test.describe('with audits feature', () => {

  test.beforeEach(async({ mockEnvs }) => {
    await mockEnvs(ENVS_MAP.hasContractAuditReports);
  });

  test('no audits', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:contract_security_audits', { items: [] }, { pathParams: { hash: addressMock.contract.hash } });
    const props = {
      data: contractMock.verified,
      isLoading: false,
      addressData: addressMock.contract,
    };
    const component = await render(<ContractDetailsInfo { ...props }/>);

    await expect(component).toHaveScreenshot();
  });

  test('has audits', async({ render, mockApiResponse }) => {
    await mockApiResponse('general:contract_security_audits', contractAudits, { pathParams: { hash: addressMock.contract.hash } });
    const props = {
      data: contractMock.verified,
      isLoading: false,
      addressData: addressMock.contract,
    };
    const component = await render(<ContractDetailsInfo { ...props }/>);

    await expect(component).toHaveScreenshot();
  });
});
