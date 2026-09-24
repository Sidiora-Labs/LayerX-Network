import { Box } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import { VERIFIED_CONTRACTS_COUNTERS, VERIFIED_CONTRACTS_COUNTERS_MICROSERVICE } from 'stubs/contract';
import StatsWidget from 'ui/shared/stats/StatsWidget';

const VerifiedContractsCounters = () => {
  const multichainContext = useMultichainContext();

  const chainConfig = multichainContext?.chain.app_config || config;
  const isStatsFeatureEnabled = chainConfig.features.stats.isEnabled;

  const countersStatsQuery = useApiQuery('stats:pages_contracts', {
    queryOptions: {
      enabled: isStatsFeatureEnabled,
      placeholderData: isStatsFeatureEnabled ? VERIFIED_CONTRACTS_COUNTERS_MICROSERVICE : undefined,
    },
  });

  const countersApiQuery = useApiQuery('general:verified_contracts_counters', {
    queryOptions: {
      enabled: !isStatsFeatureEnabled,
      placeholderData: !isStatsFeatureEnabled ? VERIFIED_CONTRACTS_COUNTERS : undefined,
    },
  });

  if (!(isStatsFeatureEnabled ? countersStatsQuery.data : countersApiQuery.data)) {
    return null;
  }

  const isLoading = isStatsFeatureEnabled ? countersStatsQuery.isPlaceholderData : countersApiQuery.isPlaceholderData;

  const contractsCount = isStatsFeatureEnabled ? countersStatsQuery.data?.total_contracts?.value : countersApiQuery.data?.smart_contracts;
  const newContractsCount = isStatsFeatureEnabled ? countersStatsQuery.data?.new_contracts_24h?.value : countersApiQuery.data?.new_smart_contracts_24h;

  const verifiedContractsCount = isStatsFeatureEnabled ?
    countersStatsQuery.data?.total_verified_contracts?.value :
    countersApiQuery.data?.verified_smart_contracts;
  const newVerifiedContractsCount = isStatsFeatureEnabled ?
    countersStatsQuery.data?.new_verified_contracts_24h?.value :
    countersApiQuery.data?.new_verified_smart_contracts_24h;

  return (
    <Box columnGap={ 3 } rowGap={ 3 } mb={ 6 } display="grid" gridTemplateColumns={{ base: '1fr', lg: 'repeat(2, 1fr)' }}>
      <StatsWidget
        label="Total contracts"
        value={ Number(contractsCount).toLocaleString() }
        diff={ newContractsCount }
        diffFormatted={ Number(newContractsCount).toLocaleString() }
        isLoading={ isLoading }
        // there is no stats for contracts growth for now
        // href={ config.features.stats.isEnabled ? { pathname: '/stats/[id]', query: { id: 'contractsGrowth' } } : undefined }
      />
      <StatsWidget
        label="Verified contracts"
        value={ Number(verifiedContractsCount).toLocaleString() }
        diff={ newVerifiedContractsCount }
        diffFormatted={ Number(newVerifiedContractsCount).toLocaleString() }
        isLoading={ isLoading }
        href={
          chainConfig.features.stats.isEnabled ?
            {
              pathname: '/stats/[id]',
              query: { id: 'verifiedContractsGrowth', ...(multichainContext?.chain.id ? { chain_id: multichainContext.chain.id } : {}) },
            } :
            undefined
        }
      />
    </Box>
  );
};

export default VerifiedContractsCounters;
