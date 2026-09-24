import { Box, Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { TabItemRegular } from 'toolkit/components/AdaptiveTabs/types';

import getCheckedSummedAddress from 'lib/address/getCheckedSummedAddress';
import useApiQuery from 'lib/api/useApiQuery';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import * as contract from 'lib/multichain/contract';
import getQueryParamString from 'lib/router/getQueryParamString';
import { ADDRESS } from 'stubs/multichain';
import RoutedTabs from 'toolkit/components/RoutedTabs/RoutedTabs';
import { CONTRACT_TAB_IDS } from 'ui/address/contract/utils';
import AddressQrCode from 'ui/address/details/AddressQrCode';
import ClusterChainsPopover from 'ui/multichain/components/ClusterChainsPopover';
import TextAd from 'ui/shared/ad/TextAd';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EnsEntity from 'ui/shared/entities/ens/EnsEntity';
import PageTitle from 'ui/shared/Page/PageTitle';

import MultichainAddressEnsDomains from './header/MultichainAddressEnsDomains';
import MultichainAddressCoinBalanceHistory from './MultichainAddressCoinBalanceHistory';
import MultichainAddressContract from './MultichainAddressContract';
import MultichainAddressInternalTxs from './MultichainAddressInternalTxs';
import MultichainAddressLogs from './MultichainAddressLogs';
import MultichainAddressPortfolio from './MultichainAddressPortfolio';
import MultichainAddressTokenTransfers, { ADDRESS_MULTICHAIN_TOKEN_TRANSFERS_TAB_IDS } from './MultichainAddressTokenTransfers';
import MultichainAddressTxs, { ADDRESS_MULTICHAIN_TXS_TAB_IDS } from './MultichainAddressTxs';

const TABS_PRESERVED_PARAMS = [ 'chain_id' ];

const MultichainAddress = () => {
  const router = useRouter();

  const hash = getQueryParamString(router.query.hash);

  const addressQuery = useApiQuery('multichainAggregator:address', {
    pathParams: { hash },
    queryOptions: {
      placeholderData: ADDRESS,
    },
  });

  throwOnResourceLoadError(addressQuery);

  const isLoading = addressQuery.isPlaceholderData;
  const chainData = Object.values(addressQuery.data?.chain_infos ?? {});
  const isContractSomewhere = chainData.some((chainInfo) => chainInfo.is_contract);
  const isContract = contract.isContract(addressQuery.data);
  const isVerified = contract.isVerified(addressQuery.data);

  const checkSummedHash = React.useMemo(() => {
    if (isLoading) {
      return getCheckedSummedAddress(hash);
    }
    return addressQuery.data?.hash ?? getCheckedSummedAddress(hash);
  }, [ hash, addressQuery.data?.hash, isLoading ]);

  const tabs: Array<TabItemRegular> = React.useMemo(() => {
    return [
      {
        id: 'portfolio',
        title: 'Portfolio',
        component: <MultichainAddressPortfolio addressData={ addressQuery.data } isLoading={ isLoading }/>,
      },
      isContractSomewhere && {
        id: 'contract',
        title: 'Contract',
        component: <MultichainAddressContract addressHash={ checkSummedHash } data={ addressQuery.data } isLoading={ isLoading }/>,
        subTabs: CONTRACT_TAB_IDS,
      },
      {
        id: 'txs',
        title: 'Transactions',
        component: <MultichainAddressTxs addressData={ addressQuery.data } isLoading={ isLoading }/>,
        subTabs: ADDRESS_MULTICHAIN_TXS_TAB_IDS,
      },
      {
        id: 'token_transfers',
        title: 'Token transfers',
        component: <MultichainAddressTokenTransfers addressData={ addressQuery.data } isLoading={ isLoading }/>,
        subTabs: ADDRESS_MULTICHAIN_TOKEN_TRANSFERS_TAB_IDS,
      },
      {
        id: 'internal_txs',
        title: 'Internal txns',
        component: <MultichainAddressInternalTxs addressData={ addressQuery.data } isLoading={ isLoading }/>,
      },
      {
        id: 'coin_balance_history',
        title: 'Coin balance history',
        component: <MultichainAddressCoinBalanceHistory addressData={ addressQuery.data } isLoading={ isLoading }/>,
      },
      isContractSomewhere && {
        id: 'logs',
        title: 'Logs',
        component: <MultichainAddressLogs addressData={ addressQuery.data } isLoading={ isLoading }/>,
      },
    ].filter(Boolean);
  }, [ addressQuery.data, isLoading, isContractSomewhere, checkSummedHash ]);

  const titleSecondRow = (
    <Flex alignItems="center" w="100%" columnGap={ 2 } rowGap={ 2 } flexWrap={{ base: 'wrap', lg: 'nowrap' }}>
      { addressQuery.data?.domains?.[0] && (
        <EnsEntity
          domain={ addressQuery.data?.domains[0].name }
          protocol={ addressQuery.data?.domains[0].protocol }
          isLoading={ isLoading }
          variant="subheading"
          noLink
          mr={ 1 }
          maxW="300px"
        />
      ) }
      <AddressEntity
        address={{
          ...addressQuery.data,
          hash: checkSummedHash,
          name: '',
          ens_domain_name: '',
          implementations: null,
          is_contract: isContract,
          is_verified: isVerified,
        }}
        isLoading={ isLoading }
        variant="subheading"
        noLink
        icon={{
          shield: { name: 'pie_chart', isLoading },
        }}
      />
      <AddressQrCode hash={ checkSummedHash } isLoading={ isLoading }/>
      <Box ml="auto"/>
      <MultichainAddressEnsDomains
        mainDomain={ addressQuery.data?.domains?.[0] }
        isLoading={ isLoading }
        hash={ checkSummedHash }
      />
      <ClusterChainsPopover addressHash={ checkSummedHash } data={ addressQuery.data } isLoading={ isLoading }/>
    </Flex>
  );

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle
        title={ `${ isContract ? 'Contract' : 'Address' } details` }
        isLoading={ isLoading }
        secondRow={ titleSecondRow }
      />
      <RoutedTabs tabs={ tabs } isLoading={ isLoading } preservedParams={ TABS_PRESERVED_PARAMS }/>
    </>
  );
};

export default React.memo(MultichainAddress);
