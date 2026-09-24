import { pickBy } from 'es-toolkit';
import React from 'react';

import type { AddressFromToFilter } from 'types/api/address';
import type { TokenType } from 'types/api/token';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import { getAdvancedFilterTypes } from 'ui/advancedFilter/constants';
import AdvancedFilterLink from 'ui/shared/links/AdvancedFilterLink';

interface Props {
  isLoading?: boolean;
  address: string;
  typeFilter: Array<TokenType>;
  directionFilter: AddressFromToFilter;
  chainData?: ClusterChainConfig;
}

const AddressAdvancedFilterLink = ({ isLoading, address, typeFilter, directionFilter, chainData }: Props) => {
  const isInitialLoading = useIsInitialLoading(isLoading);
  const multichainContext = useMultichainContext();

  const chainConfig = chainData?.app_config || multichainContext?.chain.app_config || config;

  if (!chainConfig?.features?.advancedFilter.isEnabled) {
    return null;
  }

  const advancedFilterTypes = getAdvancedFilterTypes(chainConfig);

  const queryParams = pickBy({
    to_address_hashes_to_include: !directionFilter || directionFilter === 'to' ? [ address ] : undefined,
    from_address_hashes_to_include: !directionFilter || directionFilter === 'from' ? [ address ] : undefined,
    transaction_types: typeFilter.length > 0 ?
      typeFilter :
      advancedFilterTypes.filter((type) => type.id !== 'coin_transfer').map((type) => type.id),
  }, (value) => value !== undefined);

  const routeParams = (chainData ? { chain: chainData } : undefined) ?? multichainContext;

  return (
    <AdvancedFilterLink
      query={ queryParams }
      routeParams={ routeParams || undefined }
      loading={ isInitialLoading }
    />
  );
};

export default React.memo(AddressAdvancedFilterLink);
