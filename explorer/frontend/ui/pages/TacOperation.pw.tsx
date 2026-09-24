import React from 'react';

import * as tac from '@blockscout/tac-operation-lifecycle-types';

import * as tacOperationMock from 'mocks/operations/tac';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import TacOperation from './TacOperation';

test('base view +@dark-mode +@mobile', async({ render, mockTextAd, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.tac);
  await mockTextAd();
  await mockApiResponse('tac:operation', tacOperationMock.tacOperation, {
    pathParams: { id: tacOperationMock.tacOperation.operation_id },
  });

  const component = await render(
    <TacOperation/>,
    { hooksConfig: {
      router: {
        query: { id: tacOperationMock.tacOperation.operation_id },
        isReady: true,
      },
    } },
  );
  await component.getByRole('button', { name: 'Collected in TON' }).click();
  await component.getByRole('button', { name: 'Included in TON consensus' }).click();
  await component.getByRole('button', { name: 'Executed in TON' }).click();
  await expect(component).toHaveScreenshot();
});

test('pending operation', async({ render, mockTextAd, mockApiResponse, mockEnvs }) => {
  await mockEnvs(ENVS_MAP.tac);
  await mockTextAd();
  await mockApiResponse('tac:operation', {
    ... tacOperationMock.tacOperation,
    type: tac.OperationType.PENDING,
  }, {
    pathParams: { id: tacOperationMock.tacOperation.operation_id },
  });

  const component = await render(
    <TacOperation/>,
    { hooksConfig: {
      router: {
        query: { id: tacOperationMock.tacOperation.operation_id },
        isReady: true,
      },
    } },
  );
  await expect(component).toHaveScreenshot();
});
