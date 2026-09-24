import { Box, HStack, VStack } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import multichainConfig from 'configs/multichain';
import useApiQuery from 'lib/api/useApiQuery';
import { MultichainProvider } from 'lib/contexts/multichain';
import { Link } from 'toolkit/chakra/link';
import HeroBanner from 'ui/home/HeroBanner';

import ChainWidget from './ChainWidget';
import LatestTxs from './LatestTxs';
import Stats from './Stats';

const MultichainHome = () => {
  const chains = multichainConfig()?.chains;

  const chainMetricsQuery = useApiQuery('multichainAggregator:chain_metrics');

  return (
    <Box as="main">
      <HeroBanner/>
      <Stats/>
      <LatestTxs/>
      { chains && chains.length > 0 && (
        <VStack rowGap={ 3 } alignItems="stretch">
          <HStack gap={{ base: 2, lg: 3 }} w="100%" flexWrap="wrap" alignItems="stretch">
            { chains.slice(0, 4).map((chain) => (
              <MultichainProvider key={ chain.id } chainId={ chain.id }>
                <ChainWidget
                  data={ chain }
                  isLoading={ chainMetricsQuery.isLoading }
                  metrics={ chainMetricsQuery.data?.items.find((metric) => metric.chain_id === chain.id) }
                />
              </MultichainProvider>
            )) }
          </HStack>
          <Link textStyle="sm" justifyContent="center" href={ route({ pathname: '/ecosystems' }) }>View all chains</Link>
        </VStack>
      ) }
    </Box>
  );
};

export default React.memo(MultichainHome);
