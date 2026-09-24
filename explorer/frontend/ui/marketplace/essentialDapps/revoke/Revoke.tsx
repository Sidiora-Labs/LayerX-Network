import { Flex, Text } from '@chakra-ui/react';
import { getEnsAddress } from '@wagmi/core';
import { useRouter } from 'next/router';
import React, { useMemo, useCallback, useEffect, useState } from 'react';
import { isAddress } from 'viem';
import { mainnet } from 'viem/chains';
import { normalize } from 'viem/ens';
import { useAccount } from 'wagmi';

import config from 'configs/app';
import essentialDappsChainsConfig from 'configs/essential-dapps-chains';
import useIsMobile from 'lib/hooks/useIsMobile';
import * as mixpanel from 'lib/mixpanel';
import getQueryParamString from 'lib/router/getQueryParamString';
import { useQueryParams } from 'lib/router/useQueryParams';
import useWeb3Wallet from 'lib/web3/useWallet';
import wagmiConfig from 'lib/web3/wagmiConfig';
import { Button } from 'toolkit/chakra/button';
import { EmptyState } from 'toolkit/chakra/empty-state';
import { Tooltip } from 'toolkit/chakra/tooltip';

import AddressEntity from './components/AddressEntity';
import ChainSelect from './components/ChainSelect';
import Content from './components/Content';
import SearchInput from './components/SearchInput';
import StartScreen from './components/StartScreen';
import useApprovalsQuery from './hooks/useApprovalsQuery';
import useCoinBalanceQuery from './hooks/useCoinBalanceQuery';

const feature = config.features.marketplace;
const dappConfig = feature.isEnabled ? feature.essentialDapps?.revoke : undefined;

const defaultChainId = (
  dappConfig?.chains.includes(config.chain.id as string) ?
    config.chain.id :
    dappConfig?.chains[0]
) as string;

const Revoke = () => {
  const router = useRouter();
  const { updateQuery } = useQueryParams();
  const chainIdFromQuery: string | undefined = getQueryParamString(router.query.chainId);
  const addressFromQuery = getQueryParamString(router.query.address);
  const [ selectedChainId, setSelectedChainId ] = useState<Array<string>>([ chainIdFromQuery || defaultChainId ]);
  const { address: connectedAddress } = useAccount();
  const [ searchAddress, setSearchAddress ] = useState(addressFromQuery || '');
  const [ searchInputValue, setSearchInputValue ] = useState('');

  const selectedChain = essentialDappsChainsConfig()?.chains.find((chain) => chain.id === selectedChainId[0]);

  const approvalsQuery = useApprovalsQuery(selectedChain, searchAddress);
  const coinBalanceQuery = useCoinBalanceQuery(selectedChain, searchAddress);
  const web3Wallet = useWeb3Wallet({ source: 'Essential dapps' });
  const isMobile = useIsMobile();

  const isValidAddress = useMemo(
    () => isAddress(searchAddress.toLowerCase()),
    [ searchAddress ],
  );

  const isAddressMatch = useMemo(
    () => searchAddress.toLowerCase() === connectedAddress?.toLowerCase(),
    [ searchAddress, connectedAddress ],
  );

  const handleChainValueChange = useCallback(({ value }: { value: Array<string> }) => {
    setSelectedChainId(value);
    mixpanel.logEvent(mixpanel.EventTypes.PAGE_WIDGET, {
      Type: 'Chain switch',
      Info: value[0],
      Source: 'Revoke essential dapp',
    });
    updateQuery({ chainId: value[0] }, true);
  }, [ updateQuery ]);

  const handleSearch = useCallback(async(address: string) => {
    if (address.endsWith('.eth')) {
      try {
        const ensAddress = await getEnsAddress(wagmiConfig.config, {
          chainId: mainnet.id,
          name: normalize(address),
        });
        if (ensAddress) {
          address = ensAddress.toLowerCase();
        }
      } catch {}
    }
    setSearchAddress(address);
    setSearchInputValue('');
    if (isAddress(address.toLowerCase())) {
      updateQuery({ address }, true);
    }
  }, [ updateQuery ]);

  const handleAddressClick = useCallback(
    (address: string) => () => {
      handleSearch(address);
    },
    [ handleSearch ],
  );

  useEffect(() => {
    if (connectedAddress && !searchAddress) {
      handleSearch(connectedAddress);
    }
  }, [ connectedAddress, handleSearch, searchAddress ]);

  let content = <StartScreen/>;

  if (searchAddress && selectedChainId) {
    content = isValidAddress ? (
      <Content
        searchAddress={ searchAddress }
        selectedChain={ selectedChain }
        isAddressMatch={ isAddressMatch }
        coinBalanceQuery={ coinBalanceQuery }
        approvalsQuery={ approvalsQuery }
      />
    ) : (
      <EmptyState
        description={ `The input "${ searchAddress }" is not correct. Enter a correct 0x address to search` }
      />
    );
  }

  return (
    <Flex flexDir="column" w="full" gap={{ base: 6, lg: 12 }}>
      <Flex flexDir="column" w="full" gap={ 3 }>
        <SearchInput
          value={ searchInputValue }
          onChange={ setSearchInputValue }
          onSubmit={ handleSearch }
        />
        <Flex flexDir={{ base: 'column', md: 'row' }} gap={ 3 } justifyContent="space-between">
          <Flex
            gap={ 3 }
            alignItems="center"
            h="32px"
            overflowX="auto"
            css={{
              '-ms-overflow-style': 'none',
              scrollbarWidth: 'none',
              '&::-webkit-scrollbar': {
                display: 'none',
              },
            }}
          >
            <Text textStyle="sm" fontWeight="500" color="text.secondary">
              Examples
            </Text>
            { [
              '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
              '0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7',
              '0xf6B6F07862A02C85628B3A9688beae07fEA9C863',
            ].slice(0, isMobile ? 2 : undefined).map((address) => (
              <Button
                key={ address }
                variant="plain"
                size="sm"
                p={ 0 }
                onClick={ handleAddressClick(address) }
              >
                <AddressEntity
                  address={{ hash: address }}
                  truncation="constant"
                  noLink
                  noCopy
                  textStyle="sm"
                  fontWeight="600"
                />
              </Button>
            )) }
          </Flex>
          <Flex gap={ 3 } w={{ base: 'full', md: 'auto' }}>
            { connectedAddress ? (
              <Flex gap={ 2 } alignItems="center" flexShrink={ 0 }>
                <Text textStyle="sm" fontWeight="500" color="text.secondary">My wallet</Text>
                <Tooltip content="Click to see your approvals" disableOnMobile>
                  <Button
                    variant="plain"
                    size="sm"
                    p={ 0 }
                    onClick={ handleAddressClick(connectedAddress) }
                  >
                    <AddressEntity
                      address={{ hash: connectedAddress }}
                      truncation="constant"
                      noTooltip
                      noLink
                      noCopy
                      textStyle="sm"
                      fontWeight="600"
                    />
                  </Button>
                </Tooltip>
              </Flex>
            ) : (
              <Button
                size="sm"
                variant="outline"
                onClick={ web3Wallet.connect }
                loading={ web3Wallet.isOpen }
                loadingText="Connect wallet"
                flexShrink={ 0 }
              >
                Connect wallet
              </Button>
            ) }
            <ChainSelect
              value={ selectedChainId }
              onValueChange={ handleChainValueChange }
              mode="default"
            />
          </Flex>
        </Flex>
      </Flex>
      { content }
    </Flex>
  );
};

export default Revoke;
