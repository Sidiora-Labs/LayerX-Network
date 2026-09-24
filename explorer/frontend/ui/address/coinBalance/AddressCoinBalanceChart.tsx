import BigNumber from 'bignumber.js';
import React from 'react';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { currencyUnits } from 'lib/units';
import { ChartWidget } from 'toolkit/components/charts/ChartWidget';
import { useChartsConfig } from 'ui/shared/chart/config';

interface Props {
  addressHash: string;
}

const AddressCoinBalanceChart = ({ addressHash }: Props) => {
  const { data, isPending, isError } = useApiQuery('general:address_coin_balance_chart', {
    pathParams: { hash: addressHash },
  });
  const chartsConfig = useChartsConfig();

  const charts = React.useMemo(() => {
    if (!data) {
      return [];
    }

    return [
      {
        id: 'balance',
        name: 'Value',
        items: data.items.map(({ date, value }) => ({
          date: new Date(date),
          value: BigNumber(value).div(10 ** config.chain.currency.decimals).toNumber(),
        })),
        charts: chartsConfig,
        units: currencyUnits.ether,
      },
    ];
  }, [ chartsConfig, data ]);

  return (
    <ChartWidget
      isError={ isError }
      title="Balances"
      charts={ charts }
      isLoading={ isPending }
      h="300px"
      emptyText={ data?.days ? `Insufficient data for the past ${ data.days } days` : undefined }
    />
  );
};

export default React.memo(AddressCoinBalanceChart);
