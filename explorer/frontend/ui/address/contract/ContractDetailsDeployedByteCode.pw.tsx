import React from 'react';

import * as addressMock from 'mocks/address/address';
import { test, expect } from 'playwright/lib';

import ContractDetailsDeployedByteCode from './ContractDetailsDeployedByteCode';

test('scilla decoded bytecode', async({ render, mockEnvs }) => {
  await mockEnvs([
    [ 'NEXT_PUBLIC_VIEWS_CONTRACT_DECODED_BYTECODE_ENABLED', 'true' ],
  ]);
  const component = await render(
    <ContractDetailsDeployedByteCode
      // eslint-disable-next-line max-len
      bytecode="0x7363696c6c615f76657273696f6e20300a6c6962726172792053534e4c69737450726f78790a6c6574207a65726f203d2055696e7431323820300a6c6574206f6e655f6d7367203d0a66756e20286d3a204d65737361676529203d3e0a"
      isLoading={ false }
      addressData={{ ...addressMock.contract, is_verified: false }}
      showVerificationButton
    />,
  );
  await expect(component).toHaveScreenshot();
});
