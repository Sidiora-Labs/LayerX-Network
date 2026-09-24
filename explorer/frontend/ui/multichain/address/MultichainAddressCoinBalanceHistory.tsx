import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import multichainConfig from 'configs/multichain';
import getSocketUrl from 'lib/api/getSocketUrl';
import { MultichainProvider } from 'lib/contexts/multichain';
import useRoutedChainSelect from 'lib/multichain/useRoutedChainSelect';
import { SocketProvider } from 'lib/socket/context';
import AddressCoinBalance from 'ui/address/AddressCoinBalance';
import ChainSelect from 'ui/multichain/components/ChainSelect';

import getAvailableChainIds from './getAvailableChainIds';

const QUERY_PRESERVED_PARAMS = [ 'tab', 'hash' ];

interface Props {
  addressData: multichain.GetAddressResponse | undefined;
  isLoading: boolean;
}

const MultichainAddressCoinBalanceHistory = ({ addressData, isLoading }: Props) => {
  const chainIds = React.useMemo(() => getAvailableChainIds(addressData), [ addressData ]);

  const chainSelect = useRoutedChainSelect({ persistedParams: QUERY_PRESERVED_PARAMS, chainIds, isLoading });
  const chainConfig = multichainConfig()?.chains.find(({ id }) => id === chainSelect.value?.[0]);

  if (chainIds.length === 0) {
    return <p>There is no coin balance history.</p>;
  }

  return (
    <>
      <ChainSelect
        value={ chainSelect.value }
        onValueChange={ chainSelect.onValueChange }
        chainIds={ chainIds }
        loading={ isLoading }
        mb={ 3 }
      />
      { !isLoading && chainSelect.value?.[0] && chainConfig && (
        <MultichainProvider chainId={ chainSelect.value?.[0] }>
          <SocketProvider url={ getSocketUrl(chainConfig?.app_config) }>
            <AddressCoinBalance key={ chainSelect.value?.[0] }/>
          </SocketProvider>
        </MultichainProvider>
      ) }
    </>
  );
};

export default React.memo(MultichainAddressCoinBalanceHistory);
