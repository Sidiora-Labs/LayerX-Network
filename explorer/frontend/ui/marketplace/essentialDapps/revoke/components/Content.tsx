import { Flex, Text, Separator } from '@chakra-ui/react';
import React, { useState, useCallback, useMemo } from 'react';

import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { AllowanceType } from 'types/client/revoke';

import { route } from 'nextjs/routes';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Badge } from 'toolkit/chakra/badge';
import { Heading } from 'toolkit/chakra/heading';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import TokenLogoPlaceholder from 'ui/shared/TokenLogoPlaceholder';

import type useApprovalsQuery from '../hooks/useApprovalsQuery';
import type useCoinBalanceQuery from '../hooks/useCoinBalanceQuery';
import AddressEntity from './AddressEntity';
import Approvals from './Approvals';

type Props = {
  searchAddress: string;
  selectedChain: EssentialDappsChainConfig | undefined;
  isAddressMatch: boolean;
  coinBalanceQuery: ReturnType<typeof useCoinBalanceQuery>;
  approvalsQuery: ReturnType<typeof useApprovalsQuery>;
};

const Content = ({
  searchAddress,
  selectedChain,
  isAddressMatch,
  coinBalanceQuery,
  approvalsQuery,
}: Props) => {
  const isMobile = useIsMobile();
  const [ hiddenApprovals, setHiddenApprovals ] = useState<Array<AllowanceType>>([]);

  const approvals = useMemo(() => {
    return approvalsQuery.data?.filter((approval) => !hiddenApprovals.includes(approval));
  }, [ approvalsQuery.data, hiddenApprovals ]);

  const totalValueAtRiskUsd = useMemo(() => {
    if (approvalsQuery.isPlaceholderData || !approvals) return 0;

    const maxValues: Record<`0x${ string }`, number> = {};

    approvals.forEach((item) => {
      const { address, valueAtRiskUsd } = item;

      if (!valueAtRiskUsd) return;

      if (
        maxValues[address] === undefined ||
        valueAtRiskUsd > maxValues[address]
      ) {
        maxValues[address] = valueAtRiskUsd;
      }
    });

    const sum = Object.values(maxValues).reduce((sum, val) => sum + val, 0);

    return Number(sum.toFixed(2)).toLocaleString();
  }, [ approvalsQuery.isPlaceholderData, approvals ]);

  const hideApproval = useCallback((approval: AllowanceType) => {
    setHiddenApprovals((prev) => [ ...prev, approval ]);
  }, []);

  return (
    <Flex flexDir="column" w="full">
      <Flex
        flexDir={{ base: 'column', lg: 'row' }}
        gap={ 2 }
        mt={ -2 }
        pt={ 2 }
        pb={ 6 }
        position={ !isMobile && approvals?.length ? 'sticky' : 'unset' }
        top={ 0 }
        zIndex="1"
        bg={{ _light: 'white', _dark: 'black' }}
      >
        <Flex
          flexDir="column"
          alignItems="flex-start"
          flex={ 1 }
          bg={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
          gap={ 3 }
          p={ 6 }
          borderRadius="base"
        >
          <Flex
            flexDir={{ base: 'column', md: 'row' }}
            gap={ 3 }
            alignItems={{ base: 'flex-start', md: 'center' }}
            justifyContent={{ base: 'flex-start', md: 'space-between' }}
            w="full"
            flexWrap="wrap"
          >
            <Flex gap={ 2 } alignItems="center">
              <AddressEntity
                address={{ hash: searchAddress }}
                truncation="constant"
                textStyle={{ base: 'heading.sm', lg: 'heading.md' }}
                fontWeight="500"
                icon={{ size: isMobile ? undefined : 30 }}
                noLink
              />
              <Tooltip content="Connect a wallet to revoke approvals" disabled={ isAddressMatch } disableOnMobile>
                <Badge colorPalette={ isAddressMatch ? 'green' : 'gray' }>
                  { isAddressMatch ? 'Connected' : 'Not connected' }
                </Badge>
              </Tooltip>
            </Flex>
          </Flex>
          <Flex gap={ 3 } alignItems="center" flexWrap="wrap">
            <Skeleton
              loading={ coinBalanceQuery.isPlaceholderData }
              as={ Flex }
              gap={ 3 }
            >
              { (coinBalanceQuery.isPlaceholderData ||
                coinBalanceQuery.data) && (
                <>
                  <Flex gap={ 2 } alignItems="center" ml={{ base: 0, lg: '5px' }}>
                    <Image
                      src={ coinBalanceQuery.data.coinImage }
                      alt={ coinBalanceQuery.data.symbol }
                      boxSize={ 5 }
                      fallback={ <TokenLogoPlaceholder/> }
                    />
                    <Text textStyle="sm" fontWeight="500">
                      { coinBalanceQuery.data.balance }{ ' ' }
                      { coinBalanceQuery.data.symbol }
                    </Text>
                  </Flex>
                  <Text textStyle="sm" fontWeight="500" color="text.secondary">
                    ${ coinBalanceQuery.data.balanceUsd }
                  </Text>
                </>
              ) }
            </Skeleton>
            <Link
              href={ route({ pathname: '/address/[hash]', query: { hash: searchAddress } }, { chain: selectedChain, external: true }) }
              external
              textStyle="sm"
              fontWeight="500"
              noIcon
            >
              View details
            </Link>
          </Flex>
        </Flex>
        <Flex
          w={{ base: 'full', lg: '400px' }}
          bg={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
          p={ 6 }
          borderRadius="base"
        >
          <Flex
            flexDir="column"
            flex={ 1 }
            justifyContent="center"
            alignItems="center"
            gap={ 2 }
          >
            <Text textStyle="sm" fontWeight="500" color="text.secondary">
              Total approvals
            </Text>
            <Skeleton
              loading={ approvalsQuery.isPlaceholderData }
              minW="40px"
              textAlign="center"
            >
              <Heading level="3">
                { approvals?.length || 0 }
              </Heading>
            </Skeleton>
          </Flex>
          <Separator
            orientation="vertical"
            mx={{ base: 4, md: 8 }}
          />
          <Flex
            flexDir="column"
            flex={ 1 }
            justifyContent="center"
            alignItems="center"
            gap={ 2 }
          >
            <Text textStyle="sm" fontWeight="500" color="text.secondary">
              Total value at risk
            </Text>
            <Skeleton
              loading={ approvalsQuery.isPlaceholderData }
              minW="40px"
              textAlign="center"
            >
              <Heading level="3">
                ${ totalValueAtRiskUsd }
              </Heading>
            </Skeleton>
          </Flex>
        </Flex>
      </Flex>
      <Approvals
        selectedChain={ selectedChain }
        approvals={ approvals || [] }
        isLoading={ approvalsQuery.isPlaceholderData }
        isAddressMatch={ isAddressMatch }
        hideApproval={ hideApproval }
      />
    </Flex>
  );
};

export default Content;
