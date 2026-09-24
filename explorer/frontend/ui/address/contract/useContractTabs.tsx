import type { Channel } from 'phoenix';
import React from 'react';

import type { Address } from 'types/api/address';
import type { ClusterChainConfig } from 'types/multichain';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import * as stubs from 'stubs/contract';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import ContractDetails from 'ui/address/contract/ContractDetails';
import ContractMethodsCustom from 'ui/address/contract/methods/ContractMethodsCustom';
import ContractMethodsMudSystem from 'ui/address/contract/methods/ContractMethodsMudSystem';
import ContractMethodsProxy from 'ui/address/contract/methods/ContractMethodsProxy';
import ContractMethodsRegular from 'ui/address/contract/methods/ContractMethodsRegular';

import type { CONTRACT_MAIN_TAB_IDS } from './utils';
import { CONTRACT_DETAILS_TAB_IDS } from './utils';

interface ContractTab {
  id: typeof CONTRACT_MAIN_TAB_IDS[number] | Array<typeof CONTRACT_MAIN_TAB_IDS[number]>;
  title: string;
  component: React.JSX.Element;
  subTabs?: Array<string>;
}

interface ReturnType {
  tabs: Array<ContractTab>;
  isLoading: boolean;
  isPartiallyVerified?: boolean;
}

interface Props {
  addressData: Address | undefined;
  isEnabled: boolean;
  hasMudTab?: boolean;
  channel?: Channel;
  chain?: ClusterChainConfig;
}

export default function useContractTabs({ addressData, isEnabled, hasMudTab, channel, chain }: Props): ReturnType {
  const contractQuery = useApiQuery('general:contract', {
    pathParams: { hash: addressData?.hash },
    queryOptions: {
      enabled: isEnabled && addressData?.is_contract,
      refetchOnMount: false,
      placeholderData: addressData?.is_verified ? stubs.CONTRACT_CODE_VERIFIED : stubs.CONTRACT_CODE_UNVERIFIED,
    },
    chain,
  });

  const mudSystemsQuery = useApiQuery('general:mud_systems', {
    pathParams: { hash: addressData?.hash },
    queryOptions: {
      enabled: Boolean(isEnabled && hasMudTab && addressData?.is_contract),
      refetchOnMount: false,
      placeholderData: stubs.MUD_SYSTEMS,
    },
  });

  const verifiedImplementations = React.useMemo(() => {
    return addressData?.implementations?.filter(({ name, address_hash: addressHash }) => name && addressHash && addressHash !== addressData?.hash) || [];
  }, [ addressData?.hash, addressData?.implementations ]);

  return React.useMemo(() => {

    if (!addressData?.is_contract) {
      return {
        tabs: [
          {
            id: 'contract_code' as const,
            title: 'Code',
            component: <p>This address is not a contract on this chain.</p>,
          },
        ],
        isLoading: false,
        isPartiallyVerified: undefined,
      };
    }

    return {
      tabs: [
        addressData && {
          id: 'contract_code' as const,
          title: 'Code',
          component: <ContractDetails mainContractQuery={ contractQuery } channel={ channel } addressData={ addressData }/>,
          subTabs: CONTRACT_DETAILS_TAB_IDS as unknown as Array<string>,
        },
        contractQuery.data?.abi && {
          id: [ 'read_write_contract' as const, 'read_contract' as const, 'write_contract' as const ],
          title: 'Read/Write contract',
          component: <ContractMethodsRegular abi={ contractQuery.data.abi } isLoading={ contractQuery.isPlaceholderData }/>,
        },
        verifiedImplementations.length > 0 && {
          id: [ 'read_write_proxy' as const, 'read_proxy' as const, 'write_proxy' as const ],
          title: 'Read/Write proxy',
          component: (
            <ContractMethodsProxy
              implementations={ verifiedImplementations }
              isLoading={ contractQuery.isPlaceholderData }
              proxyType={ addressData?.proxy_type }
              conflictingImplementations={ contractQuery.data?.conflicting_implementations ?? undefined }
            />
          ),
        },
        config.features.account.isEnabled && {
          id: [ 'read_write_custom_methods' as const, 'read_custom_methods' as const, 'write_custom_methods' as const ],
          title: 'Custom ABI',
          component: <ContractMethodsCustom isLoading={ contractQuery.isPlaceholderData }/>,
        },
        hasMudTab && {
          id: 'mud_system' as const,
          title: 'MUD System',
          component: mudSystemsQuery.isPlaceholderData ?
            <ContentLoader/> :
            <ContractMethodsMudSystem items={ mudSystemsQuery.data?.items ?? [] }/>,
        },
      ].filter(Boolean),
      isLoading: contractQuery.isPlaceholderData,
      isPartiallyVerified: !contractQuery.isPlaceholderData ? contractQuery.data?.is_partially_verified || undefined : undefined,
    };
  }, [
    addressData,
    contractQuery,
    channel,
    verifiedImplementations,
    hasMudTab,
    mudSystemsQuery.isPlaceholderData,
    mudSystemsQuery.data?.items,
  ]);
}
