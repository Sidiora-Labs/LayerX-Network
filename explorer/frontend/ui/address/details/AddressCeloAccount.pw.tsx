import React from 'react';

import * as addressMock from 'mocks/address/address';
import { test, expect } from 'playwright/lib';
import { Container } from 'ui/shared/DetailedInfo/DetailedInfo';

import AddressCeloAccount from './AddressCeloAccount';

const DATA = {
  name: 'CLabs Validator #2 on alfajores',
  type: 'validator',
  locked_celo: '10000000000000000000000',
  // eslint-disable-next-line max-len
  metadata_url: 'https://storage.googleapis.com/clabs_validator_metadata/alfajores/validator-alfajores-0x050f34537F5b2a00B9B9C752Cb8500a3fcE3DA7d-metadata.json',
  nonvoting_locked_celo: '10000000000000000000000',
  vote_signer_address: addressMock.withName,
  validator_signer_address: addressMock.withEns,
  attestation_signer_address: addressMock.withoutName,
};

test('default view +@mobile', async({ render }) => {
  const component = await render(
    <Container>
      <AddressCeloAccount data={ DATA }/>
    </Container>,
  );
  await component.getByText('View details').click();
  await expect(component).toHaveScreenshot();
});
