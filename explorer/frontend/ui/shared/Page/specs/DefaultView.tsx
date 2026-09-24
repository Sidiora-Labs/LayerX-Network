import React from 'react';

import type { TokenInfo } from 'types/api/token';

import * as addressMock from 'mocks/address/address';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import * as TokenEntity from 'ui/shared/entities/token/TokenEntity';
import EntityTags from 'ui/shared/EntityTags/EntityTags';
import IconSvg from 'ui/shared/IconSvg';
import NetworkExplorers from 'ui/shared/NetworkExplorers';

import PageTitle from '../PageTitle';

const DefaultView = () => {
  const tokenData: TokenInfo = {
    address_hash: '0x363574E6C5C71c343d7348093D84320c76d5Dd29',
    circulating_market_cap: '117629601.61913824',
    type: 'ERC-20',
    symbol: 'SHAAAAAAAAAAAAA',
    name: null,
    decimals: '18',
    holders_count: '1',
    exchange_rate: null,
    total_supply: null,
    icon_url: 'https://example.com/logo.png',
    reputation: 'ok',
  };

  const contentAfter = (
    <>
      <IconSvg name="certified" color="green.500" boxSize={ 6 } cursor="pointer"/>
      <EntityTags
        tags={ [
          { slug: 'example', name: 'Example label', tagType: 'custom', ordinal: 0 },
        ] }
        flexGrow={ 1 }
      />
    </>
  );

  const secondRow = (
    <>
      <AddressEntity
        address={{ ...addressMock.token, name: '' }}
        variant="subheading"
      />
      <NetworkExplorers type="token" pathParam={ addressMock.hash } ml="auto"/>
    </>
  );

  return (
    <PageTitle
      title="Shavukha Token (SHVKH) token"
      beforeTitle={ (
        <TokenEntity.Icon
          token={ tokenData }
          variant="heading"
        />
      ) }
      contentAfter={ contentAfter }
      secondRow={ secondRow }
    />
  );
};

export default DefaultView;
