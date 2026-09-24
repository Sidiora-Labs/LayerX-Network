import React from 'react';

import * as tac from '@blockscout/tac-operation-lifecycle-types';

import { test, expect } from 'playwright/lib';

import TacOperationStatus from './TacOperationStatus';

const STATUSES: Array<tac.OperationType> = [
  tac.OperationType.TON_TAC_TON,
  tac.OperationType.TAC_TON,
  tac.OperationType.TON_TAC,
  tac.OperationType.ERROR,
  tac.OperationType.PENDING,
];

test.use({ viewport: { width: 200, height: 50 } });

STATUSES.forEach((status) => {
  test(`${ status }`, async({ render }) => {
    const component = await render(<TacOperationStatus status={ status }/>);
    await expect(component).toHaveScreenshot();
  });
});
