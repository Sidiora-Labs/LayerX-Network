import { Box, HStack, Stat } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ClusterChainConfig } from 'types/multichain';

import useAddChainClick from 'lib/web3/useAddChainClick';
import useProvider from 'lib/web3/useProvider';
import { WALLETS_INFO } from 'lib/web3/wallets';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  data: multichain.ChainMetrics;
  chainInfo: ClusterChainConfig | undefined;
  isLoading?: boolean;
}

const MultichainEcosystemsTableItem = ({ data, isLoading, chainInfo }: Props) => {

  const { data: { wallet } = {} } = useProvider();
  const walletIcon = wallet ? WALLETS_INFO[wallet].icon : undefined;

  const handleAddToWalletClick = useAddChainClick({ source: 'Chain widget' });

  const activeAddresses = {
    value: data.active_accounts?.current_full_week ? Number(data.active_accounts.current_full_week) : 0,
    delta: data.active_accounts?.wow_diff_percent ? Number(data.active_accounts.wow_diff_percent) : 0,
  };

  const newAddresses = {
    value: data.new_addresses?.current_full_week ? Number(data.new_addresses.current_full_week) : 0,
    delta: data.new_addresses?.wow_diff_percent ? Number(data.new_addresses.wow_diff_percent) : 0,
  };

  const dailyTransactions = {
    value: data.daily_transactions?.current_full_week ? Number(data.daily_transactions.current_full_week) : 0,
    delta: data.daily_transactions?.wow_diff_percent ? Number(data.daily_transactions.wow_diff_percent) : 0,
  };

  return (
    <TableRow alignItems="top">
      <TableCell verticalAlign="middle">
        <HStack gap={ 3 }>
          <HStack>
            <ChainIcon data={ chainInfo } isLoading={ isLoading }/>
            <Link
              href={ chainInfo?.explorer_url }
              fontWeight={ 700 }
              external
              loading={ isLoading }
              maxW="calc(100% - 28px)"
            >
              <TruncatedText text={ chainInfo?.name ?? 'Unknown chain' } loading={ isLoading }/>
            </Link>
          </HStack>
          <HStack gap={ 0 } flexShrink={ 0 }>
            <Skeleton loading={ isLoading } color="text.secondary"><span>{ data.chain_id }</span></Skeleton>
            <CopyToClipboard text={ String(data.chain_id) } isLoading={ isLoading }/>
          </HStack>
        </HStack>
      </TableCell>
      <TableCell verticalAlign="middle">
        <HStack>
          <Skeleton loading={ isLoading }>
            <span>{ activeAddresses.value.toLocaleString() }</span>
          </Skeleton>
          { activeAddresses.delta ? (
            <Skeleton loading={ isLoading }>
              <Stat.Root positive={ activeAddresses.delta > 0 }>
                <Stat.ValueText>
                  { activeAddresses.delta.toFixed(2) }%
                </Stat.ValueText>
                { activeAddresses.delta > 0 ? <Stat.UpIndicator/> : <Stat.DownIndicator/> }
              </Stat.Root>
            </Skeleton>
          ) : null }
        </HStack>
      </TableCell>
      <TableCell verticalAlign="middle">
        <HStack>
          <Skeleton loading={ isLoading }>
            <span>{ newAddresses.value.toLocaleString() }</span>
          </Skeleton>
          { newAddresses.delta ? (
            <Skeleton loading={ isLoading }>
              <Stat.Root positive={ newAddresses.delta > 0 }>
                <Stat.ValueText>
                  { newAddresses.delta.toFixed(2) }%
                </Stat.ValueText>
                { newAddresses.delta > 0 ? <Stat.UpIndicator/> : <Stat.DownIndicator/> }
              </Stat.Root>
            </Skeleton>
          ) : null }
        </HStack>
      </TableCell>
      <TableCell verticalAlign="middle">
        <HStack>
          <Skeleton loading={ isLoading }>
            <span>{ dailyTransactions.value.toLocaleString() }</span>
          </Skeleton>
          { dailyTransactions.delta ? (
            <Skeleton loading={ isLoading }>
              <Stat.Root positive={ dailyTransactions.delta > 0 }>
                <Stat.ValueText>
                  { dailyTransactions.delta.toFixed(2) }%
                </Stat.ValueText>
                { dailyTransactions.delta > 0 ? <Stat.UpIndicator/> : <Stat.DownIndicator/> }
              </Stat.Root>
            </Skeleton>
          ) : null }
        </HStack>
      </TableCell>
      <TableCell verticalAlign="middle">
        <HStack justifyContent="space-between">
          <Skeleton loading={ isLoading }>
            <span>{ Number(data.tps ?? 0).toLocaleString(undefined, { maximumFractionDigits: 2 }) }</span>
          </Skeleton>
          { walletIcon ? (
            <Tooltip content="Add to wallet">
              <IconButton
                onClick={ handleAddToWalletClick }
                size="md"
                variant="icon_background"
                bgColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }}
                loadingSkeleton={ isLoading }
              >
                <IconSvg name={ walletIcon } boxSize={ 5 }/>
              </IconButton>
            </Tooltip>
          ) : <Box boxSize={ 8 }/> }
        </HStack>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(MultichainEcosystemsTableItem);
