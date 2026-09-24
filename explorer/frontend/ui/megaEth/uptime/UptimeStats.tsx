import { Box } from '@chakra-ui/react';
import React from 'react';

import type { UptimeRealTimeData } from 'types/api/megaEth';

import StatsWidget from 'ui/shared/stats/StatsWidget';

interface Props {
  realtimeData: UptimeRealTimeData | null;
}

const UptimeStats = ({ realtimeData }: Props) => {

  return (
    <Box
      columnGap={ 2 }
      rowGap={ 2 }
      mb={ 8 }
      display="grid"
      gridTemplateColumns={{ base: '1fr', lg: 'repeat(4, 1fr)' }}
    >
      <StatsWidget
        label="Current TPS"
        hint="Number of transactions processed per second on the network"
        value={ realtimeData ? Number(realtimeData.instant_tps).toLocaleString() : '-' }
      />
      <StatsWidget
        label="MGas/s"
        hint="Number of computational gas consumed per second on the network"
        value={ realtimeData ? Number(realtimeData.instant_mgas_per_second).toLocaleString(undefined, { maximumFractionDigits: 2 }) : '-' }
      />
      <StatsWidget
        label="Block height"
        hint="Number of blocks created since the genesis block"
        value={ realtimeData ? Number(realtimeData.latest_mini_block_id).toLocaleString() : '-' }
      />
      <StatsWidget
        label="Block time"
        hint="Time taken by the sequencer to produce a new block"
        valuePostfix=" ms"
        value={ realtimeData ? Number(realtimeData.instant_mini_block_interval).toLocaleString(undefined, { maximumFractionDigits: 2 }) : '-' }
      />
    </Box>
  );
};

export default React.memo(UptimeStats);
