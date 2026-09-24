import { Box, HStack, VStack } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ClusterChainConfig } from 'types/multichain';

import useIsMobile from 'lib/hooks/useIsMobile';
import useAddChainClick from 'lib/web3/useAddChainClick';
import useProvider from 'lib/web3/useProvider';
import { WALLETS_INFO } from 'lib/web3/wallets';
import { Heading } from 'toolkit/chakra/heading';
import { IconButton } from 'toolkit/chakra/icon-button';
import { LinkBox, LinkOverlay } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import IconSvg from 'ui/shared/IconSvg';
import RollupStageBadge from 'ui/snippets/navigation/RollupStageBadge';

interface Props {
  data: ClusterChainConfig;
  isLoading: boolean;
  metrics: multichain.ChainMetrics | undefined;
}

const ChainWidget = ({ data, isLoading, metrics }: Props) => {
  const isMobile = useIsMobile();
  const { data: { wallet } = {} } = useProvider();
  const walletIcon = wallet ? WALLETS_INFO[wallet].icon : undefined;
  const handleAddToWalletClick = useAddChainClick({ source: 'Chain widget' });

  const chainStats = (
    <VStack gap={ 2 } alignItems="flex-start" fontWeight={ 500 }>
      <HStack gap={ 2 }>
        <Skeleton loading={ isLoading } color="text.secondary">
          <span>Chain ID</span>
        </Skeleton>
        <Skeleton loading={ isLoading }>{ data.id }</Skeleton>
        <CopyToClipboard text={ String(data.id) } ml={ 0 } isLoading={ isLoading }/>
      </HStack>
      { metrics?.active_accounts?.current_full_week && (
        <HStack gap={ 2 }>
          <Skeleton loading={ isLoading } color="text.secondary">
            <span>Active accounts</span>
          </Skeleton>
          <Skeleton loading={ isLoading }>{ Number(metrics.active_accounts.current_full_week).toLocaleString() }</Skeleton>
        </HStack>
      ) }
      { metrics?.tps && (
        <HStack gap={ 2 }>
          <Skeleton loading={ isLoading } color="text.secondary">
            <span>TPS</span>
          </Skeleton>
          <Skeleton loading={ isLoading }>{ metrics.tps }</Skeleton>
        </HStack>
      ) }
    </VStack>
  );

  if (isMobile) {
    return (
      <LinkBox
        bgColor={{ _light: 'rgba(246, 246, 248, 0.5)', _dark: 'whiteAlpha.50' }}
        borderRadius="xl"
        border="1px solid"
        borderColor={{ _light: 'blackAlpha.200', _dark: 'whiteAlpha.200' }}
        p={ 4 }
        flexBasis="100%"
        textStyle="sm"
        overflow="hidden"
      >
        <HStack justifyContent="space-between" mb={ 2 }>
          <HStack minW="0">
            <ChainIcon data={ data } boxSize={ 5 } isLoading={ isLoading } noTooltip/>
            <Heading textStyle="heading.sm" as="h3" minW="0">
              <LinkOverlay href={ data.explorer_url } external loading={ isLoading } _groupHover={{ color: 'hover' }}>
                <Box overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
                  { data.name }
                </Box>
              </LinkOverlay>
            </Heading>
          </HStack>
          <RollupStageBadge chainConfig={ data.app_config } isLoading={ isLoading }/>
        </HStack>
        { chainStats }
      </LinkBox>
    );
  }

  return (
    <LinkBox
      className="group"
      // as the designer said, for the light theme "there is no gray color that suits well, so we use a custom one"
      bgColor={{ _light: 'rgba(246, 246, 248, 0.5)', _dark: 'whiteAlpha.50' }}
      borderRadius="xl"
      border="1px solid"
      borderColor={{ _light: 'blackAlpha.200', _dark: 'whiteAlpha.200' }}
      _hover={ !isLoading ? {
        bgColor: { _light: 'gray.50', _dark: 'whiteAlpha.100' },
        borderColor: 'hover',
      } : undefined }
      p={ 6 }
      flexBasis="calc((100% - 3 * 12px) / 4)"
      textStyle="sm"
      overflow="hidden"
    >
      <HStack justifyContent="space-between">
        <ChainIcon data={ data } boxSize="30px" isLoading={ isLoading } noTooltip/>
        { walletIcon && (
          <Tooltip content="Add to wallet">
            <IconButton
              onClick={ handleAddToWalletClick }
              size="md"
              variant="icon_background"
              zIndex={ 1 }
              bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
              loadingSkeleton={ isLoading }
            >
              <IconSvg name={ walletIcon } boxSize={ 5 }/>
            </IconButton>
          </Tooltip>
        ) }
      </HStack>
      <Heading my={ 3 } textStyle="heading.md" as="h3">
        <LinkOverlay href={ data.explorer_url } external loading={ isLoading } _groupHover={{ color: 'hover' }}>
          <Box overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
            { data.name }
          </Box>
        </LinkOverlay>
      </Heading>
      <RollupStageBadge chainConfig={ data.app_config } isLoading={ isLoading } mb={ 2.5 }/>
      { chainStats }
    </LinkBox>
  );
};

export default React.memo(ChainWidget);
