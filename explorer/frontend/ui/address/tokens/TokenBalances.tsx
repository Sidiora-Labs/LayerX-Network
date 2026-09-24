import { Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { currencyUnits } from 'lib/units';
import { ZERO } from 'toolkit/utils/consts';
import { thinsp } from 'toolkit/utils/htmlEntities';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import IconSvg from 'ui/shared/IconSvg';
import NativeTokenIcon from 'ui/shared/NativeTokenIcon';
import calculateUsdValue from 'ui/shared/value/calculateUsdValue';

import { getTokensTotalInfo } from '../utils/tokenUtils';
import useFetchTokens from '../utils/useFetchTokens';
import TokenBalancesItem from './TokenBalancesItem';

const TokenBalances = () => {
  const router = useRouter();

  const hash = router.query.hash?.toString();

  const addressQuery = useApiQuery('general:address', {
    pathParams: { hash },
    queryOptions: { enabled: Boolean(hash), refetchOnMount: false },
  });

  const tokenQuery = useFetchTokens({ hash });

  if (addressQuery.isError || tokenQuery.isError) {
    return <DataFetchAlert/>;
  }

  const addressData = addressQuery.data;
  const { valueStr: nativeValue, usdBn: nativeUsd } = calculateUsdValue({
    amount: addressData?.coin_balance || '0',
    exchangeRate: addressData?.exchange_rate,
    decimals: String(config.chain.currency.decimals),
  });

  const tokensInfo = getTokensTotalInfo(tokenQuery.data);
  const prefix = tokensInfo.isOverflow ? `>${ thinsp }` : '';
  const totalUsd = nativeUsd.plus(tokensInfo.usd);
  const tokensNumText = tokensInfo.num > 0 ?
    `${ prefix }${ tokensInfo.num } ${ tokensInfo.num > 1 ? 'tokens' : 'token' }` :
    '0';

  return (
    <Flex columnGap={ 3 } rowGap={ 3 } mt={{ base: '6px', lg: 0 }} flexDirection={{ base: 'column', lg: 'row' }}>
      <TokenBalancesItem
        name="Net Worth"
        value={ addressData?.exchange_rate ? `${ prefix }$${ totalUsd.toFormat(2) }` : 'N/A' }
        isLoading={ addressQuery.isPending || tokenQuery.isPending }
        icon={ <IconSvg name="wallet" boxSize="20px" flexShrink={ 0 } color="icon.primary"/> }
      />
      <TokenBalancesItem
        name={ `${ currencyUnits.ether } Balance` }
        value={ `${ nativeValue } ${ currencyUnits.ether }` }
        valueSecondary={ !nativeUsd.eq(ZERO) ? `$${ nativeUsd.toFormat(2) }` : '' }
        isLoading={ addressQuery.isPending || tokenQuery.isPending }
        icon={ <NativeTokenIcon boxSize="20px"/> }
      />
      <TokenBalancesItem
        name="Tokens"
        value={ tokensNumText }
        valueSecondary={ `${ prefix }$${ tokensInfo.usd.toFormat(2) }` }
        isLoading={ addressQuery.isPending || tokenQuery.isPending }
        icon={ <IconSvg name="tokens" boxSize="20px" flexShrink={ 0 } color="icon.primary"/> }
      />
    </Flex>
  );
};

export default TokenBalances;
