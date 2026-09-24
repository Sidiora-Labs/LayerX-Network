import { Flex } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { SmartContract } from 'types/api/contract';

import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import getQueryParamString from 'lib/router/getQueryParamString';
import { Button } from 'toolkit/chakra/button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import CustomAbiModal from 'ui/customAbi/CustomAbiModal/CustomAbiModal';
import RawDataSnippet from 'ui/shared/RawDataSnippet';
import AuthGuard from 'ui/snippets/auth/guard/AuthGuard';
import useIsAuth from 'ui/snippets/auth/useIsAuth';

import ContractAbi from './ContractAbi';
import ContractMethodsAlerts from './ContractMethodsAlerts';
import ContractMethodsContainer from './ContractMethodsContainer';
import ContractMethodsFilters from './ContractMethodsFilters';
import useMethodsFilters from './useMethodsFilters';
import { formatAbi } from './utils';

interface Props {
  isLoading?: boolean;
}

const ContractMethodsCustom = ({ isLoading: isLoadingProp }: Props) => {

  const modal = useDisclosure();
  const router = useRouter();
  const queryClient = useQueryClient();

  const tab = getQueryParamString(router.query.tab);
  const addressHash = getQueryParamString(router.query.hash);

  const isAuth = useIsAuth();

  const customAbiQuery = useApiQuery('general:custom_abi', {
    queryOptions: {
      enabled: !isLoadingProp && isAuth,
      refetchOnMount: false,
    },
  });

  const contractQueryData = queryClient.getQueryData<SmartContract>(getResourceKey('general:contract', { pathParams: { hash: addressHash } }));

  const isLoading = isLoadingProp || (isAuth && customAbiQuery.isLoading);

  const currentInfo = customAbiQuery.data?.find((item) => item.contract_address_hash.toLowerCase() === addressHash.toLowerCase());
  const modalData = currentInfo ?? (contractQueryData ? {
    name: contractQueryData.name || '',
    contract_address_hash: addressHash,
  } : undefined);

  const abi = React.useMemo(() => formatAbi(currentInfo?.abi || []), [ currentInfo ]);
  const filters = useMethodsFilters({ abi });

  const updateButton = React.useMemo(() => {
    return (
      <Skeleton loading={ isLoading } ml="auto" mr="3" borderRadius="base">
        <Button
          size="sm"
          variant="outline"
          onClick={ modal.onOpen }
        >
          Update
        </Button>
      </Skeleton>
    );
  }, [ isLoading, modal.onOpen ]);

  return (
    <Flex flexDir="column" rowGap={ 6 }>
      { currentInfo ? (
        <>
          <ContractMethodsAlerts
            isCustomAbi
            isLoading={ isLoading }
          />
          <RawDataSnippet
            data={ JSON.stringify(abi) }
            title="Contract ABI"
            textareaMaxHeight="150px"
            isLoading={ isLoading }
            rightSlot={ updateButton }
          />
          <ContractMethodsFilters
            defaultMethodType={ filters.methodType }
            defaultSearchTerm={ filters.searchTerm }
            onChange={ filters.onChange }
            isLoading={ isLoading }
          />
          <ContractMethodsContainer isLoading={ isLoading } isEmpty={ abi.length === 0 } type={ filters.methodType }>
            <ContractAbi abi={ abi } tab={ tab } addressHash={ addressHash } visibleItems={ filters.visibleItems }/>
          </ContractMethodsContainer>
        </>
      ) : (
        <>
          <Skeleton loading={ isLoading }>
            Add custom ABIs for this contract and access when logged into your account. Helpful for debugging,
            functional testing and contract interaction.
          </Skeleton>
          <AuthGuard onAuthSuccess={ modal.onOpen }>
            { ({ onClick }) => (
              <Skeleton loading={ isLoading } w="fit-content">
                <Button
                  size="sm"
                  onClick={ onClick }
                >
                  Add custom ABI
                </Button>
              </Skeleton>
            ) }
          </AuthGuard>
        </>
      ) }
      <CustomAbiModal { ...modal } data={ modalData }/>
    </Flex>
  );
};

export default React.memo(ContractMethodsCustom);
