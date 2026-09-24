import type { UseQueryResult } from '@tanstack/react-query';
import React from 'react';

import type * as bens from '@blockscout/bens-types';

import type { ResourceError } from 'lib/api/resources';
import * as ensDomainMock from 'mocks/ens/domain';
import { test, expect } from 'playwright/lib';

import AddressEnsDomains from './AddressEnsDomains';

const ADDRESS_HASH = ensDomainMock.ensDomainA.owner?.hash as string;

test('base view', async({ render, page, mockAssetResponse }) => {
  const query = {
    data: {
      items: [
        ensDomainMock.ensDomainA,
        ensDomainMock.ensDomainB,
        ensDomainMock.ensDomainC,
        ensDomainMock.ensDomainD,
      ],
      next_page_params: null,
    },
    isPending: false,
    isError: false,
  } as unknown as UseQueryResult<bens.LookupAddressResponse, ResourceError<unknown>>;
  await mockAssetResponse(ensDomainMock.ensDomainA.protocol?.icon_url as string, './playwright/mocks/image_s.jpg');

  const component = await render(
    <AddressEnsDomains
      query={ query }
      addressHash={ ADDRESS_HASH }
      mainDomainName={ ensDomainMock.ensDomainA.name }
    />,
  );
  await component.getByLabel('Address domains').click();
  await expect(page).toHaveScreenshot({ clip: { x: 0, y: 0, width: 550, height: 350 } });
});
