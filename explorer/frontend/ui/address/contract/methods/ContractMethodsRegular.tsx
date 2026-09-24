import { Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';
import type { Abi } from 'viem';

import getQueryParamString from 'lib/router/getQueryParamString';

import ContractAbi from './ContractAbi';
import ContractMethodsAlerts from './ContractMethodsAlerts';
import ContractMethodsContainer from './ContractMethodsContainer';
import ContractMethodsFilters from './ContractMethodsFilters';
import useMethodsFilters from './useMethodsFilters';
import { formatAbi } from './utils';

interface Props {
  abi: Abi;
  isLoading?: boolean;
}

const ContractMethodsRegular = ({ abi, isLoading }: Props) => {

  const router = useRouter();

  const tab = getQueryParamString(router.query.tab);
  const addressHash = getQueryParamString(router.query.hash);

  const formattedAbi = React.useMemo(() => formatAbi(abi), [ abi ]);
  const filters = useMethodsFilters({ abi: formattedAbi });

  return (
    <Flex flexDir="column" rowGap={ 6 }>
      <ContractMethodsAlerts isLoading={ isLoading }/>
      <ContractMethodsFilters
        defaultMethodType={ filters.methodType }
        defaultSearchTerm={ filters.searchTerm }
        onChange={ filters.onChange }
        isLoading={ isLoading }
      />
      <ContractMethodsContainer isLoading={ isLoading } isEmpty={ formattedAbi.length === 0 } type={ filters.methodType }>
        <ContractAbi abi={ formattedAbi } tab={ tab } addressHash={ addressHash } visibleItems={ filters.visibleItems }/>
      </ContractMethodsContainer>
    </Flex>
  );
};

export default React.memo(ContractMethodsRegular);
