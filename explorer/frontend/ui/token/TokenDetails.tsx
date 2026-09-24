import { chakra } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import BigNumber from 'bignumber.js';
import { useRouter } from 'next/router';
import React, { useCallback } from 'react';
import { scroller } from 'react-scroll';

import type { TokenInfo } from 'types/api/token';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useApiQuery from 'lib/api/useApiQuery';
import { useMultichainContext } from 'lib/contexts/multichain';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import useIsMounted from 'lib/hooks/useIsMounted';
import { isConfidentialTokenType } from 'lib/token/tokenTypes';
import { TOKEN_COUNTERS } from 'stubs/token';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import type { TokenTabs } from 'ui/pages/Token';
import AppActionButton from 'ui/shared/AppActionButton/AppActionButton';
import useAppActionData from 'ui/shared/AppActionButton/useAppActionData';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoSponsoredItem from 'ui/shared/DetailedInfo/DetailedInfoSponsoredItem';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import AssetValue from 'ui/shared/value/AssetValue';

import TokenNftMarketplaces from './TokenNftMarketplaces';

interface Props {
  tokenQuery: UseQueryResult<TokenInfo, ResourceError<unknown>>;
}

const TokenDetails = ({ tokenQuery }: Props) => {
  const router = useRouter();
  const isMounted = useIsMounted();

  const hash = router.query.hash?.toString();

  const multichainContext = useMultichainContext();
  const chainSlug = multichainContext?.chain?.slug;

  const tokenCountersQuery = useApiQuery('general:token_counters', {
    pathParams: { hash },
    queryOptions: { enabled: Boolean(router.query.hash), placeholderData: TOKEN_COUNTERS },
  });

  const appActionData = useAppActionData(hash);

  const changeUrlAndScroll = useCallback((tab: TokenTabs) => () => {

    router.push(
      chainSlug ?
        { pathname: '/chain/[chain_slug]/token/[hash]', query: { hash: hash || '', tab, chain_slug: chainSlug } } :
        { pathname: '/token/[hash]', query: { hash: hash || '', tab } },
      undefined,
      { shallow: true },
    );
    scroller.scrollTo('token-tabs', {
      duration: 500,
      smooth: true,
    });
  }, [ chainSlug, hash, router ]);

  const countersItem = useCallback((item: 'token_holders_count' | 'transfers_count') => {
    const itemValue = tokenCountersQuery.data?.[item];
    if (!itemValue) {
      return 'N/A';
    }
    if (itemValue === '0') {
      return itemValue;
    }

    const tab: TokenTabs = item === 'token_holders_count' ? 'holders' : 'token_transfers';

    return (
      <Link onClick={ changeUrlAndScroll(tab) } loading={ tokenCountersQuery.isPlaceholderData }>
        { Number(itemValue).toLocaleString() }
      </Link>
    );
  }, [ tokenCountersQuery.data, tokenCountersQuery.isPlaceholderData, changeUrlAndScroll ]);

  throwOnResourceLoadError(tokenQuery);

  if (!isMounted) {
    return null;
  }

  const {
    exchange_rate: exchangeRate,
    total_supply: totalSupply,
    circulating_market_cap: marketCap,
    decimals,
    symbol,
    type,
    zilliqa,
  } = tokenQuery.data || {};

  return (
    <DetailedInfo.Container>
      { zilliqa?.zrc2_address_hash && (
        <>
          <DetailedInfo.ItemLabel
            hint="ZRC-2 address of the token"
            isLoading={ tokenQuery.isPlaceholderData }
          >
            ZRC-2 Address
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ tokenQuery.isPlaceholderData } display="inline-block">
              <AddressEntity address={{ hash: zilliqa.zrc2_address_hash }} isLoading={ tokenQuery.isPlaceholderData }/>
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { exchangeRate && (
        <>
          <DetailedInfo.ItemLabel
            hint="Price per token on the exchanges"
            isLoading={ tokenQuery.isPlaceholderData }
          >
            Price
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ tokenQuery.isPlaceholderData } display="inline-block">
              <span>{ `$${ Number(exchangeRate).toLocaleString(undefined, { minimumSignificantDigits: 4 }) }` }</span>
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { marketCap && (
        <>
          <DetailedInfo.ItemLabel
            hint="Circulating supply * Price"
            isLoading={ tokenQuery.isPlaceholderData }
          >
            Market cap
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ tokenQuery.isPlaceholderData } display="inline-block">
              <span>{ `$${ BigNumber(marketCap).toFormat() }` }</span>
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }
      { type && !isConfidentialTokenType(type) && (
        <>
          <DetailedInfo.ItemLabel
            hint="The total amount of tokens issued"
            isLoading={ tokenQuery.isPlaceholderData }
          >
            Max total supply
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue
            alignSelf="center"
            wordBreak="break-word"
            whiteSpace="pre-wrap"
          >
            <AssetValue
              amount={ totalSupply }
              asset={ <chakra.span maxW="50%" overflow="hidden" textOverflow="ellipsis"> { symbol }</chakra.span> }
              accuracy={ 3 }
              decimals={ decimals ?? '0' }
              loading={ tokenQuery.isPlaceholderData }
              w="100%"
            />
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfo.ItemLabel
        hint="Number of accounts holding the token"
        isLoading={ tokenQuery.isPlaceholderData }
      >
        Holders
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ tokenCountersQuery.isPlaceholderData }>
          { countersItem('token_holders_count') }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        hint="Number of transfer for the token"
        isLoading={ tokenQuery.isPlaceholderData }
      >
        Transfers
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ tokenCountersQuery.isPlaceholderData }>
          { countersItem('transfers_count') }
        </Skeleton>
      </DetailedInfo.ItemValue>

      { decimals && (
        <>
          <DetailedInfo.ItemLabel
            hint="Number of digits that come after the decimal place when displaying token value"
            isLoading={ tokenQuery.isPlaceholderData }
          >
            Decimals
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue>
            <Skeleton loading={ tokenQuery.isPlaceholderData } minW={ 6 }>
              { decimals }
            </Skeleton>
          </DetailedInfo.ItemValue>
        </>
      ) }

      { type !== 'ERC-20' && (
        <TokenNftMarketplaces
          hash={ hash }
          isLoading={ tokenQuery.isPlaceholderData }
          appActionData={ appActionData }
          source="NFT collection"
        />
      ) }

      { (type !== 'ERC-20' && config.UI.views.nft.marketplaces.length === 0 && appActionData) && (
        <>
          <DetailedInfo.ItemLabel
            hint="Link to the dapp"
          >
            Dapp
          </DetailedInfo.ItemLabel>
          <DetailedInfo.ItemValue
            py="1px"
          >
            <AppActionButton data={ appActionData } height="30px" source="NFT collection"/>
          </DetailedInfo.ItemValue>
        </>
      ) }

      <DetailedInfoSponsoredItem isLoading={ tokenQuery.isPlaceholderData }/>
    </DetailedInfo.Container>
  );
};

export default React.memo(TokenDetails);
