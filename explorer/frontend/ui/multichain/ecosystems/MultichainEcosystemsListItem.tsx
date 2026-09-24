import { chakra, Grid, HStack, Stat } from '@chakra-ui/react';
import React from 'react';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type { ClusterChainConfig } from 'types/multichain';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import ChainIcon from 'ui/shared/externalChains/ChainIcon';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';

interface Props {
  data: multichain.ChainMetrics;
  chainInfo: ClusterChainConfig | undefined;
  isLoading?: boolean;
}

const MultichainEcosystemsListItem = ({ data, chainInfo, isLoading }: Props) => {
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
    <ListItemMobile rowGap={ 3 } py={ 4 } fontSize="sm" alignItems="stretch">
      <HStack justifyContent="space-between" fontWeight={ 600 }>
        <HStack maxW="50%">
          <ChainIcon data={ chainInfo } isLoading={ isLoading }/>
          <Link
            href={ chainInfo?.explorer_url }
            external
            loading={ isLoading }
            maxW="100%"
          >
            <TruncatedText text={ chainInfo?.name ?? 'Unknown chain' } loading={ isLoading }/>
          </Link>
        </HStack>
        <HStack gap={ 0 } flexShrink={ 0 }>
          <Skeleton loading={ isLoading } color="text.secondary"><span>{ data.chain_id }</span></Skeleton>
          <CopyToClipboard text={ String(data.chain_id) } isLoading={ isLoading }/>
        </HStack>
      </HStack>
      <Grid gridTemplateColumns="140px 1fr" columnGap={ 2 } rowGap={ 3 }>
        <Skeleton loading={ isLoading } fontWeight={ 500 }>
          <span>Active addresses</span>
          <chakra.span color="text.secondary"> 7D</chakra.span>
        </Skeleton>
        <HStack gap={ 1 }>
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

        <Skeleton loading={ isLoading } fontWeight={ 500 }>
          <span>New addresses</span>
          <chakra.span color="text.secondary"> 7D</chakra.span>
        </Skeleton>
        <HStack gap={ 1 }>
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

        <Skeleton loading={ isLoading } fontWeight={ 500 }>
          <span>Daily txs</span>
          <chakra.span color="text.secondary"> 7D</chakra.span>
        </Skeleton>
        <HStack gap={ 1 }>
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

        <Skeleton loading={ isLoading } fontWeight={ 500 }>
          <span>TPS</span>
        </Skeleton>
        <Skeleton loading={ isLoading }>
          <span>{ Number(data.tps ?? 0).toLocaleString(undefined, { maximumFractionDigits: 2 }) }</span>
        </Skeleton>
      </Grid>
    </ListItemMobile>
  );
};

export default React.memo(MultichainEcosystemsListItem);
