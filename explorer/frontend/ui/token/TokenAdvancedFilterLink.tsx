import React from 'react';

import type { TokenInfo } from 'types/api/token';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import useIsInitialLoading from 'lib/hooks/useIsInitialLoading';
import type { LinkProps } from 'toolkit/chakra/link';
import AdvancedFilterLink from 'ui/shared/links/AdvancedFilterLink';

interface Props extends LinkProps {
  isLoading?: boolean;
  token?: TokenInfo;
}

const TokenAdvancedFilterLink = ({ isLoading, token, ...rest }: Props) => {
  const isInitialLoading = useIsInitialLoading(isLoading);
  const multichainContext = useMultichainContext();

  if (!token || !config.features.advancedFilter.isEnabled) {
    return null;
  }

  const queryParams = {
    token_contract_address_hashes_to_include: [ token.address_hash ],
  };

  return (
    <AdvancedFilterLink
      query={ queryParams }
      routeParams={{ chain: multichainContext?.chain }}
      loading={ isInitialLoading }
      { ...rest }
    />
  );
};

export default React.memo(TokenAdvancedFilterLink);
