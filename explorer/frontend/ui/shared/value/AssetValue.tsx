import type { BoxProps } from '@chakra-ui/react';
import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

import { Tag } from 'toolkit/chakra/tag';
import { thinsp } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

import type { Params as CalculateUsdValueParams } from './calculateUsdValue';
import calculateUsdValue from './calculateUsdValue';
import SimpleValue from './SimpleValue';
import { DEFAULT_ACCURACY, DEFAULT_ACCURACY_USD } from './utils';

export interface Props extends Omit<BoxProps, 'prefix' | 'suffix'>, Omit<CalculateUsdValueParams, 'amount'> {
  amount: string | null | undefined;
  asset?: React.ReactNode;
  startElement?: React.ReactNode;
  endElement?: React.ReactNode;
  noTooltip?: boolean;
  loading?: boolean;
  layout?: 'horizontal' | 'vertical';
  tooltipContent?: React.ReactNode;
  historicalExchangeRate?: string | null;
  hasExchangeRateToggle?: boolean;
}

const AssetValue = ({
  amount,
  asset,
  decimals,
  accuracy = DEFAULT_ACCURACY,
  accuracyUsd = DEFAULT_ACCURACY_USD,
  startElement,
  endElement,
  noTooltip,
  loading,
  exchangeRate,
  historicalExchangeRate,
  hasExchangeRateToggle,
  layout = 'horizontal',
  tooltipContent,
  ...rest
}: Props) => {
  const hasHistorical = Boolean(historicalExchangeRate);
  const hasCurrent = Boolean(exchangeRate);
  const hasToggle = hasHistorical && hasCurrent && hasExchangeRateToggle && amount !== '0';

  const [ showHistorical, setShowHistorical ] = React.useState(hasHistorical);

  React.useEffect(() => {
    setShowHistorical(hasHistorical);
  }, [ hasHistorical ]);

  const activeExchangeRate = showHistorical ? historicalExchangeRate : exchangeRate;

  const handleToggle = React.useCallback(() => {
    if (hasToggle) {
      setShowHistorical(prev => !prev);
    }
  }, [ hasToggle ]);

  if (amount === null || amount === undefined) {
    return <chakra.span { ...rest }>-</chakra.span>;
  }

  const { valueBn, usdBn } = calculateUsdValue({ amount, decimals, accuracy, accuracyUsd, exchangeRate: activeExchangeRate });

  if (!activeExchangeRate) {
    return (
      <SimpleValue
        value={ valueBn }
        accuracy={ accuracy }
        startElement={ startElement }
        endElement={ endElement ?? (typeof asset === 'string' ? `${ thinsp }${ asset }` : asset) }
        tooltipContent={ tooltipContent }
        noTooltip={ noTooltip }
        loading={ loading }
        { ...rest }
      />
    );
  }

  const nativeValue = (
    <SimpleValue
      value={ valueBn }
      accuracy={ accuracy }
      startElement={ startElement }
      endElement={ endElement ?? (typeof asset === 'string' ? `${ thinsp }${ asset }` : asset) }
      tooltipContent={ tooltipContent }
      noTooltip={ noTooltip }
      loading={ loading }
    />
  );

  const clockIcon = showHistorical ? (
    <IconSvg name="clock-light" boxSize="14px" color="icon.secondary"/>
  ) : undefined;

  const tooltipContentBefore = (() => {
    if (!amount || amount === '0') {
      return undefined;
    }

    // for values where historic exchange rate doesn't exist, we show current value without tooltip
    // for recent transactions, historicExchangeRate can be null, in this case we show current value with tooltip
    if (historicalExchangeRate === undefined) {
      return undefined;
    }

    return <Box>{ showHistorical ? 'Estimated value on day of txn' : 'Current value' }</Box>;
  })();

  const usdValue = hasToggle ? (
    <Tag
      size="md"
      startElement={ clockIcon }
      onClick={ handleToggle }
      loading={ loading }
      variant="clickable"
    >
      <SimpleValue
        value={ usdBn }
        accuracy={ accuracyUsd }
        prefix="$"
        tooltipContentBefore={ tooltipContentBefore }
      />
    </Tag>
  ) : (
    <SimpleValue
      value={ usdBn }
      accuracy={ accuracyUsd }
      startElement={ layout === 'horizontal' ? <span>(</span> : undefined }
      prefix="$"
      endElement={ layout === 'horizontal' ? <span>)</span> : undefined }
      tooltipContentBefore={ tooltipContentBefore }
      loading={ loading }
      color="text.secondary"
    />
  );

  return (
    <chakra.span
      display="inline-flex"
      flexDirection={ layout === 'vertical' ? 'column' : 'row' }
      alignItems={ layout === 'vertical' ? 'flex-end' : 'center' }
      maxW="100%"
      columnGap={ 1 }
      rowGap={ 1 }
      { ...rest }
    >
      { nativeValue }
      { usdValue }
    </chakra.span>
  );
};

export default React.memo(AssetValue);
