import React from 'react';

import * as solidityscanReportMock from 'mocks/contract/solidityscanReport';
import { test, expect } from 'playwright/lib';

import SolidityscanReport from './SolidityscanReport';

const addressHash = 'hash';

test('average report +@dark-mode +@mobile', async({ render, mockApiResponse, page }) => {
  await mockApiResponse(
    'general:contract_solidity_scan_report',
    solidityscanReportMock.solidityscanReportAverage,
    { pathParams: { hash: addressHash } },
  );
  const component = await render(<SolidityscanReport hash={ addressHash }/>);
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 100, height: 50 } });

  await component.getByLabel('SolidityScan score').click();

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 400, height: 500 } });
});

test('great report', async({ render, mockApiResponse, page }) => {
  await mockApiResponse(
    'general:contract_solidity_scan_report',
    solidityscanReportMock.solidityscanReportGreat,
    { pathParams: { hash: addressHash } },
  );

  const component = await render(
    <SolidityscanReport hash={ addressHash }/>,
  );

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 100, height: 50 } });

  await component.getByLabel('SolidityScan score').click();

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 400, height: 500 } });
});

test('low report', async({ render, mockApiResponse, page }) => {
  await mockApiResponse(
    'general:contract_solidity_scan_report',
    solidityscanReportMock.solidityscanReportLow,
    { pathParams: { hash: addressHash } },
  );

  const component = await render(
    <SolidityscanReport hash={ addressHash }/>,
  );

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 100, height: 50 } });

  await component.getByLabel('SolidityScan score').click();

  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 400, height: 500 } });
});
