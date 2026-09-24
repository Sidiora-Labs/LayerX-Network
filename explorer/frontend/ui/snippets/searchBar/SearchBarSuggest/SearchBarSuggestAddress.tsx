import { chakra, Box, Text, Flex, Grid } from '@chakra-ui/react';
import React from 'react';

import type { ItemsProps } from './types';
import type { SearchResultAddressOrContract, SearchResultMetadataTag } from 'types/api/search';
import type * as multichain from 'types/client/multichainAggregator';

import { toBech32Address } from 'lib/address/bech32';
import dayjs from 'lib/date/dayjs';
import highlightText from 'lib/highlightText';
import * as contract from 'lib/multichain/contract';
import { ADDRESS_REGEXP } from 'toolkit/utils/regexp';
import SearchResultEntityTag from 'ui/searchResults/SearchResultEntityTag';
import ContractCertifiedLabel from 'ui/shared/ContractCertifiedLabel';
import * as AddressEntity from 'ui/shared/entities/address/AddressEntity';
import HashStringShortenDynamic from 'ui/shared/HashStringShortenDynamic';

type Props = ItemsProps<SearchResultAddressOrContract | SearchResultMetadataTag | multichain.QuickSearchResultAddress>;

const SearchBarSuggestAddress = ({ data, isMobile, searchTerm, addressFormat }: Props) => {
  const shouldHighlightHash = ADDRESS_REGEXP.test(searchTerm);
  const hash = 'filecoin_robust_address' in data ? data.filecoin_robust_address ||
    (addressFormat === 'bech32' ? toBech32Address(data.address_hash) : data.address_hash) : data.address_hash;

  const isContract = (() => {
    if ('chain_infos' in data) {
      return contract.isContract(data);
    }
    return data.type === 'contract' || data.is_smart_contract_address;
  })();

  const isVerified = (() => {
    if ('chain_infos' in data) {
      return contract.isVerified(data);
    }
    return data.is_smart_contract_verified;
  })();

  const icon = (
    <AddressEntity.Icon
      address={{
        hash: data.address_hash,
        is_contract: isContract,
        name: '',
        is_verified: isVerified,
        ens_domain_name: null,
        implementations: null,
      }}
    />
  );
  const addressName = (() => {
    if ('chain_infos' in data) {
      return contract.getName(data);
    }
    return data.name || data.ens_info?.name;
  })();
  const expiresText = 'ens_info' in data && data.ens_info?.expiry_date ? ` (expires ${ dayjs(data.ens_info.expiry_date).fromNow() })` : '';

  const nameEl = addressName && (
    <Flex alignItems="center" overflow="hidden">
      <Text
        color="text.secondary"
        overflow="hidden"
        whiteSpace="nowrap"
        textOverflow="ellipsis"
      >
        <chakra.span fontWeight={ 500 } dangerouslySetInnerHTML={{ __html: highlightText(addressName, searchTerm) }}/>
        { 'ens_info' in data && data.ens_info && (
          data.ens_info.names_count > 1 ?
            <span> ({ data.ens_info.names_count > 39 ? '40+' : `+${ data.ens_info.names_count - 1 }` })</span> :
            <span>{ expiresText }</span>
        ) }
      </Text>
      { 'certified' in data && data.certified && <ContractCertifiedLabel boxSize={ 4 } iconSize={ 4 } ml={ 1 } flexShrink={ 0 }/> }
    </Flex>
  );
  const tagEl = data.type === 'metadata_tag' ? (
    <SearchResultEntityTag metadata={ data.metadata } addressHash={ hash } searchTerm={ searchTerm } ml={{ base: 0, lg: 'auto' }}/>
  ) : null;
  const addressEl = <HashStringShortenDynamic hash={ hash } noTooltip/>;

  if (isMobile) {
    return (
      <>
        <Flex alignItems="center">
          { icon }
          <Box
            as={ shouldHighlightHash ? 'mark' : 'span' }
            display="block"
            overflow="hidden"
            whiteSpace="nowrap"
            fontWeight={ 700 }
          >
            { addressEl }
          </Box>
        </Flex>
        <Flex alignItems="center" justifyContent="space-between" flexWrap="wrap" gap={ 2 }>
          { nameEl }
          { tagEl }
        </Flex>
      </>
    );
  }

  return (
    <Grid templateColumns="repeat(2, minmax(0, 1fr))" gap={ 2 }>
      <Flex alignItems="center" mr={ 2 } minWidth={ 0 }>
        { icon }
        <Box
          as={ shouldHighlightHash ? 'mark' : 'span' }
          display="block"
          overflow="hidden"
          whiteSpace="nowrap"
          fontWeight={ 700 }
          minWidth={ 0 }
        >
          { addressEl }
        </Box>
      </Flex>
      <Flex alignItems="center" justifyContent="space-between" gap={ 2 } minWidth={ 0 }>
        { nameEl }
        { tagEl }
      </Flex>
    </Grid>
  );
};

export default React.memo(SearchBarSuggestAddress);
