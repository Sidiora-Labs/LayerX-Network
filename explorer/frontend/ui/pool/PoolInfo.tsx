import React from 'react';

import type { Pool } from 'types/api/pools';

import { Skeleton } from 'toolkit/chakra/skeleton';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoSponsoredItem from 'ui/shared/DetailedInfo/DetailedInfoSponsoredItem';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';

type Props = {
  data: Pool;
  isPlaceholderData: boolean;
};

const PoolInfo = ({ data, isPlaceholderData }: Props) => {
  return (
    <DetailedInfo.Container>
      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="The base token in a liquidity pool pair"
      >
        Base token
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <TokenEntity
          token={{
            type: 'ERC-20',
            address_hash: data.base_token_address,
            name: data.base_token_symbol,
            symbol: data.base_token_symbol,
            icon_url: data.base_token_icon_url,
            reputation: null,
          }}
          isLoading={ isPlaceholderData }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="The quote token in a liquidity pool pair"
      >
        Quote token
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <TokenEntity
          token={{
            type: 'ERC-20',
            address_hash: data.quote_token_address,
            name: data.quote_token_symbol,
            symbol: data.quote_token_symbol,
            icon_url: data.quote_token_icon_url,
            reputation: null,
          }}
          isLoading={ isPlaceholderData }
        />
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="Fully Diluted Valuation: theoretical market cap if all tokens were in circulation"
      >
        Base token FDV
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.base_token_fully_diluted_valuation_usd ?
            `$${ Number(data.base_token_fully_diluted_valuation_usd).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }) }` :
            'N/A'
          }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="Current market capitalization of the base token"
      >
        Base token market cap
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.base_token_market_cap_usd ?
            `$${ Number(data.base_token_market_cap_usd).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }) }` :
            'N/A'
          }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="Fully Diluted Valuation: theoretical market cap if all tokens were in circulation"
      >
        Quote token FDV
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.quote_token_fully_diluted_valuation_usd ?
            `$${ Number(data.quote_token_fully_diluted_valuation_usd).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }) }` :
            'N/A'
          }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="Current market capitalization of the base token"
      >
        Quote token market cap
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.quote_token_market_cap_usd ?
            `$${ Number(data.quote_token_market_cap_usd).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }) }` :
            'N/A'
          }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="Current liquidity of the pool"
      >
        Liquidity
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          ${ Number(data.liquidity).toLocaleString(undefined, { maximumFractionDigits: 2, notation: 'compact' }) }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfo.ItemLabel
        isLoading={ isPlaceholderData }
        hint="DEX where the pool is traded"
      >
        DEX
      </DetailedInfo.ItemLabel>
      <DetailedInfo.ItemValue>
        <Skeleton loading={ isPlaceholderData }>
          { data.dex.name }
        </Skeleton>
      </DetailedInfo.ItemValue>

      <DetailedInfoSponsoredItem isLoading={ isPlaceholderData }/>
    </DetailedInfo.Container>
  );
};

export default PoolInfo;
