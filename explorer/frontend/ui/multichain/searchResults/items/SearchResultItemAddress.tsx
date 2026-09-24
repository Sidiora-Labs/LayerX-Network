import { Box } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import { route } from 'nextjs/routes';

import * as contract from 'lib/multichain/contract';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';

import SearchResultListItem from '../SearchResultListItem';

interface Props {
  data: multichain.GetAddressResponse;
  isMobile?: boolean;
}

const SearchResultItemAddress = ({ data, isMobile }: Props) => {

  const contractName = contract.getName(data);

  return (
    <SearchResultListItem
      href={ route({ pathname: '/address/[hash]', query: { hash: data.hash } }) }
    >
      <Box w={{ base: '100%', lg: '200px' }}>
        <AddressEntity
          address={{
            hash: data.hash,
            is_contract: contract.isContract(data),
            is_verified: contract.isVerified(data),
          }}
          truncation={ !isMobile ? 'constant' : 'dynamic' }
          noLink
          noCopy
          fontWeight={{ base: '600', lg: '700' }}
        />
      </Box>
      { contractName && (
        <Box color="text.secondary" _groupHover={{ color: 'inherit' }} fontWeight={{ base: '400', lg: '500' }}>
          { contractName }
        </Box>
      ) }
    </SearchResultListItem>
  );
};

export default React.memo(SearchResultItemAddress);
