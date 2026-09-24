import {
  Box,
  Flex,
  Grid,
} from '@chakra-ui/react';
import React from 'react';

import type { HomeStats } from 'types/api/stats';
import type { ExcludeUndefined } from 'types/utils';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { Link } from 'toolkit/chakra/link';
import type { TooltipProps } from 'toolkit/chakra/tooltip';
import { Tooltip } from 'toolkit/chakra/tooltip';
import Time from 'ui/shared/time/Time';

import GasInfoTooltipRow from './GasInfoTooltipRow';
import GasInfoUpdateTimer from './GasInfoUpdateTimer';

interface Props {
  children: React.ReactNode;
  data: HomeStats;
  dataUpdatedAt: number;
  placement?: ExcludeUndefined<TooltipProps['positioning']>['placement'];
}

const feature = config.features.gasTracker;

const GasInfoTooltip = ({ children, data, dataUpdatedAt, placement }: Props) => {
  if (!data.gas_prices) {
    return null;
  }

  const columnNum =
    Object.values(data.gas_prices).some((price) => price?.fiat_price) &&
    Object.values(data.gas_prices).some((price) => price?.price) &&
    feature.isEnabled && feature.units.length === 2 ?
      3 : 2;

  const content = (
    <Flex flexDir="column" textStyle="xs" rowGap={ 3 } className="dark">
      { data.gas_price_updated_at && (
        <Flex justifyContent="space-between" alignItems="center">
          <Box color="text.secondary">Last update</Box>
          <Flex color="text.secondary" justifyContent="flex-end" alignItems="center" columnGap={ 2 } ml={ 3 }>
            <Time timestamp={ data.gas_price_updated_at } format="MMM DD, HH:mm:ss"/>
            { data.gas_prices_update_in !== 0 &&
              <GasInfoUpdateTimer key={ dataUpdatedAt } startTime={ dataUpdatedAt } duration={ data.gas_prices_update_in }/> }
          </Flex>
        </Flex>
      ) }
      <Grid rowGap={ 2 } columnGap="10px" gridTemplateColumns={ `repeat(${ columnNum }, minmax(min-content, auto))` }>
        <GasInfoTooltipRow name="Fast" info={ data.gas_prices.fast }/>
        <GasInfoTooltipRow name="Normal" info={ data.gas_prices.average }/>
        <GasInfoTooltipRow name="Slow" info={ data.gas_prices.slow }/>
      </Grid>
      <Link href={ route({ pathname: '/gas-tracker' }) }>
        Gas tracker overview
      </Link>
    </Flex>
  );

  return (
    <Tooltip
      content={ content }
      positioning={{ placement }}
      lazyMount
      interactive
      showArrow={ false }
      contentProps={{ p: 4, borderRadius: 'md' }}
    >
      { children }
    </Tooltip>
  );
};

export default React.memo(GasInfoTooltip);
