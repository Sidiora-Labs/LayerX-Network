import { Box } from '@chakra-ui/react';
import React from 'react';

import multichainConfig from 'configs/multichain';
import { MultichainProvider } from 'lib/contexts/multichain';
import useRoutedChainSelect from 'lib/multichain/useRoutedChainSelect';
import useEtherscanRedirects from 'lib/router/useEtherscanRedirects';
import PageTitle from 'ui/shared/Page/PageTitle';
import ChartsWidgetsList from 'ui/stats/ChartsWidgetsList';
import NumberWidgetsList from 'ui/stats/NumberWidgetsList';
import StatsFilters from 'ui/stats/StatsFilters';
import useStats from 'ui/stats/useStats';

import ChainSelect from '../components/ChainSelect';

const MultichainStats = () => {
  useEtherscanRedirects();

  const chainIds = React.useMemo(() => {
    return multichainConfig()?.chains
      .filter((chain) => chain.app_config.features.stats.isEnabled)
      .map((chain) => chain.id);
  }, []);

  const chainSelect = useRoutedChainSelect({ chainIds });

  const chain = React.useMemo(() => {
    return multichainConfig()?.chains.find((chain) => chain.id === chainSelect.value?.[0]);
  }, [ chainSelect.value ]);

  const {
    isPlaceholderData,
    isError,
    sections,
    currentSection,
    handleSectionChange,
    interval,
    handleIntervalChange,
    handleFilterChange,
    displayedCharts,
    initialFilterQuery,
  } = useStats({ chain });

  return (
    <>
      <PageTitle title="Charts & stats"/>
      <ChainSelect
        value={ chainSelect.value }
        onValueChange={ chainSelect.onValueChange }
        chainIds={ chainIds }
        mode="default"
        mb={ 3 }
      />
      <MultichainProvider chainId={ chainSelect.value?.[0] }>
        <Box mb={{ base: 6, sm: 8 }}>
          <NumberWidgetsList/>
        </Box>

        <Box mb={{ base: 6, sm: 8 }}>
          <StatsFilters
            isLoading={ isPlaceholderData }
            initialFilterValue={ initialFilterQuery }
            sections={ sections }
            currentSection={ currentSection }
            onSectionChange={ handleSectionChange }
            interval={ interval }
            onIntervalChange={ handleIntervalChange }
            onFilterInputChange={ handleFilterChange }
          />
        </Box>

        <ChartsWidgetsList
          initialFilterQuery={ initialFilterQuery }
          isError={ isError }
          isPlaceholderData={ isPlaceholderData }
          charts={ displayedCharts }
          interval={ interval }
          sections={ sections }
          selectedSectionId={ currentSection }
        />
      </MultichainProvider>

    </>
  );
};

export default MultichainStats;
