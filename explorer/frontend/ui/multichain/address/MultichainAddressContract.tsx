import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';

import multichainConfig from 'configs/multichain';
import getSocketUrl from 'lib/api/getSocketUrl';
import useApiQuery from 'lib/api/useApiQuery';
import { MultichainProvider } from 'lib/contexts/multichain';
import useIsMobile from 'lib/hooks/useIsMobile';
import useRoutedChainSelect from 'lib/multichain/useRoutedChainSelect';
import { SocketProvider } from 'lib/socket/context';
import { ADDRESS_INFO } from 'stubs/address';
import AddressContract from 'ui/address/AddressContract';
import ChainSelect from 'ui/multichain/components/ChainSelect';

import getAvailableChainIds from './getAvailableChainIds';

const LEFT_SLOT_PROPS = {
  mr: 6,
  flexShrink: 0,
};
const QUERY_PRESERVED_PARAMS = [ 'tab', 'hash' ];

interface Props {
  addressHash: string;
  isLoading: boolean;
  data: multichain.GetAddressResponse | undefined;
}

const MultichainAddressContract = ({ addressHash, isLoading, data }: Props) => {
  const chainIds = React.useMemo(() => getAvailableChainIds(data), [ data ]);
  const chainSelect = useRoutedChainSelect({ persistedParams: QUERY_PRESERVED_PARAMS, isLoading, chainIds });
  const isMobile = useIsMobile();

  const chain = React.useMemo(() => {
    return multichainConfig()?.chains.find(({ id }) => id === chainSelect.value?.[0]);
  }, [ chainSelect.value ]);

  const addressQuery = useApiQuery('general:address', {
    pathParams: { hash: addressHash },
    queryOptions: {
      enabled: Boolean(addressHash) && !isLoading,
      placeholderData: ADDRESS_INFO,
    },
    chain,
  });

  const leftSlot = (
    <ChainSelect
      value={ chainSelect.value }
      onValueChange={ chainSelect.onValueChange }
      loading={ isLoading }
      chainIds={ chainIds }
      mode={ isMobile ? 'compact' : 'default' }
    />
  );

  return (
    <MultichainProvider key={ chainSelect.value?.[0] } chainId={ chainSelect.value?.[0] }>
      <SocketProvider url={ getSocketUrl(chain?.app_config) }>
        <AddressContract
          addressData={ addressQuery.data }
          isLoading={ isLoading || addressQuery.isPlaceholderData }
          hasMudTab={ false }
          leftSlot={ leftSlot }
          leftSlotProps={ LEFT_SLOT_PROPS }
        />
      </SocketProvider>
    </MultichainProvider>
  );
};

export default React.memo(MultichainAddressContract);
