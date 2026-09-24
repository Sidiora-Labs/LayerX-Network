import { Box, Flex } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import { HomeRpcDataContextProvider } from 'ui/home/fallbacks/rpcDataContext';
import HeroBanner from 'ui/home/HeroBanner';
import Highlights from 'ui/home/Highlights';
import ChainIndicators from 'ui/home/indicators/ChainIndicators';
import LatestArbitrumL2Batches from 'ui/home/latestBatches/LatestArbitrumL2Batches';
import LatestZkEvmL2Batches from 'ui/home/latestBatches/LatestZkEvmL2Batches';
import LatestBlocks from 'ui/home/LatestBlocks';
import Stats from 'ui/home/Stats';
import Transactions from 'ui/home/Transactions';
import AdBanner from 'ui/shared/ad/AdBanner';

const rollupFeature = config.features.rollup;

const Home = () => {
  const isMobile = useIsMobile();

  const leftWidget = (() => {
    if (rollupFeature.isEnabled && !rollupFeature.homepage.showLatestBlocks) {
      switch (rollupFeature.type) {
        case 'zkEvm':
          return <LatestZkEvmL2Batches/>;
        case 'arbitrum':
          return <LatestArbitrumL2Batches/>;
      }
    }

    return <LatestBlocks/>;
  })();

  return (
    <HomeRpcDataContextProvider>
      <Box as="main">
        <HeroBanner/>
        <Flex flexDir={{ base: 'column', lg: 'row' }} columnGap={ 2 } rowGap={ 1 } mt={ 3 } _empty={{ mt: 0 }}>
          <Stats/>
          <ChainIndicators/>
        </Flex>
        { !isMobile && config.UI.homepage.highlights && <Highlights mt={ 3 }/> }
        { isMobile && <AdBanner mt={ 6 } mx="auto" justifyContent="center" format="mobile"/> }
        <Flex mt={ 8 } direction={{ base: 'column', lg: 'row' }} columnGap={ 12 } rowGap={ 6 }>
          { leftWidget }
          <Box flexGrow={ 1 }>
            <Transactions/>
          </Box>
        </Flex>
      </Box>
    </HomeRpcDataContextProvider>
  );
};

export default Home;
