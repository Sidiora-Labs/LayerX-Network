import { Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { SmartContractMudSystemItem } from 'types/api/contract';

import useApiQuery from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';

import type { Item } from '../ContractSourceAddressSelector';
import ContractSourceAddressSelector from '../ContractSourceAddressSelector';
import ContractAbi from './ContractAbi';
import ContractMethodsAlerts from './ContractMethodsAlerts';
import ContractMethodsContainer from './ContractMethodsContainer';
import ContractMethodsFilters from './ContractMethodsFilters';
import useMethodsFilters from './useMethodsFilters';
import { formatAbi } from './utils';

interface Props {
  items: Array<SmartContractMudSystemItem>;
}

const ContractMethodsMudSystem = ({ items }: Props) => {

  const router = useRouter();

  const addressHash = getQueryParamString(router.query.hash);
  const sourceAddress = getQueryParamString(router.query.source_address);
  const tab = getQueryParamString(router.query.tab);

  const [ selectedItem, setSelectedItem ] = React.useState(items.find((item) => item.address_hash === sourceAddress) || items[0]);

  const systemInfoQuery = useApiQuery('general:mud_system_info', {
    pathParams: { hash: addressHash, system_address: selectedItem.address_hash },
    queryOptions: {
      enabled: Boolean(selectedItem?.address_hash),
      refetchOnMount: false,
    },
  });

  const handleItemSelect = React.useCallback((item: Item) => {
    setSelectedItem(item as SmartContractMudSystemItem);
  }, []);

  const abi = React.useMemo(() => formatAbi(systemInfoQuery.data?.abi || []), [ systemInfoQuery.data?.abi ]);
  const filters = useMethodsFilters({ abi });

  return (
    <Flex flexDir="column" rowGap={ 6 }>
      <ContractMethodsAlerts isLoading={ systemInfoQuery.isPending }/>
      <div>
        <ContractSourceAddressSelector
          items={ items }
          selectedItem={ selectedItem }
          onItemSelect={ handleItemSelect }
          label="System address"
          mb={ 3 }
        />
        <ContractMethodsFilters
          defaultMethodType={ filters.methodType }
          defaultSearchTerm={ filters.searchTerm }
          onChange={ filters.onChange }
        />
      </div>
      <ContractMethodsContainer
        key={ selectedItem.address_hash }
        isLoading={ systemInfoQuery.isPending }
        isEmpty={ abi.length === 0 }
        type={ filters.methodType }
        isError={ systemInfoQuery.isError }
      >
        <ContractAbi
          abi={ abi }
          tab={ tab }
          addressHash={ addressHash }
          visibleItems={ filters.visibleItems }
          sourceAddress={ selectedItem.address_hash }
        />
      </ContractMethodsContainer>
    </Flex>
  );
};

export default React.memo(ContractMethodsMudSystem);
