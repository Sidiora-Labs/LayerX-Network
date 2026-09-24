import { Box, chakra } from '@chakra-ui/react';
import React from 'react';
import type { WatchAssetParams } from 'viem';

import type { TokenInfo } from 'types/api/token';

import config from 'configs/app';
import useIsMobile from 'lib/hooks/useIsMobile';
import useRewardsActivity from 'lib/hooks/useRewardsActivity';
import * as mixpanel from 'lib/mixpanel/index';
import useProvider from 'lib/web3/useProvider';
import useSwitchOrAddChain from 'lib/web3/useSwitchOrAddChain';
import { WALLETS_INFO } from 'lib/web3/wallets';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { toaster } from 'toolkit/chakra/toaster';
import { Tooltip } from 'toolkit/chakra/tooltip';
import IconSvg from 'ui/shared/IconSvg';

function getRequestParams(token: TokenInfo, tokenId?: string): WatchAssetParams | undefined {
  switch (token.type) {
    case 'ERC-20':
      return {
        type: 'ERC20',
        options: {
          address: token.address_hash,
          symbol: token.symbol || '',
          decimals: Number(token.decimals ?? '18'),
          image: token.icon_url || '',
        },
      };
    case 'ERC-721':
    case 'ERC-1155': {
      if (!tokenId) {
        return;
      }

      return {
        type: token.type === 'ERC-721' ? 'ERC721' : 'ERC1155',
        options: {
          address: token.address_hash,
          tokenId: tokenId,
        },
      } as never; // There is no official EIP, and therefore no typings for these token types.
    }
    default:
      return;
  }
}

interface Props {
  className?: string;
  token: TokenInfo;
  tokenId?: string;
  isLoading?: boolean;
  variant?: 'icon' | 'button';
  iconSize?: number;
  chainConfig?: typeof config;
}

const AddressAddToWallet = ({ className, token, tokenId, isLoading, variant = 'icon', iconSize = 6, chainConfig }: Props) => {
  const { data: { wallet, provider } = {} } = useProvider();
  const switchOrAddChain = useSwitchOrAddChain({ chainConfig });
  const isMobile = useIsMobile();
  const { trackUsage } = useRewardsActivity();

  const feature = (chainConfig ?? config).features.web3Wallet;

  const handleClick = React.useCallback(async() => {
    if (!wallet) {
      return;
    }

    try {
      const params = getRequestParams(token, tokenId);

      if (!params) {
        throw new Error('Unsupported token type');
      }

      // switch to the correct network otherwise the token will be added to the wrong one
      await switchOrAddChain();

      const wasAdded = await provider?.request?.({
        method: 'wallet_watchAsset',
        params,
      });

      if (wasAdded) {
        toaster.success({
          title: 'Success',
          description: 'Successfully added token to your wallet',
        });

        await trackUsage('add_token');

        mixpanel.logEvent(mixpanel.EventTypes.ADD_TO_WALLET, {
          Target: 'token',
          Wallet: wallet,
          Token: token.symbol || '',
        });
      }
    } catch (error) {
      toaster.error({
        title: 'Error',
        description: (error as Error)?.message || 'Something went wrong',
      });
    }
  }, [ wallet, token, tokenId, switchOrAddChain, provider, trackUsage ]);

  if (!provider || !wallet) {
    return null;
  }

  if (isLoading) {
    return <Skeleton loading className={ className } boxSize={ iconSize } borderRadius="base"/>;
  }

  const canBeAdded = (
    // MetaMask can add NFTs now, but this is still experimental feature, and doesn't work on mobile devices
    // https://docs.metamask.io/wallet/how-to/display/tokens/#display-nfts
    wallet === 'metamask' &&
    [ 'ERC-721', 'ERC-1155' ].includes(token.type) &&
    tokenId &&
    !isMobile
  ) || token.type === 'ERC-20';

  if (!feature.isEnabled || !canBeAdded) {
    return null;
  }

  if (variant === 'button') {
    return (
      <Tooltip content={ `Add token to ${ WALLETS_INFO[wallet].name }` }>
        <IconButton
          className={ className }
          aria-label="Add token to wallet"
          variant="icon_background"
          size="md"
          onClick={ handleClick }
        >
          <IconSvg name={ WALLETS_INFO[wallet].icon }/>
        </IconButton>
      </Tooltip>
    );
  }

  return (
    <Tooltip content={ `Add token to ${ WALLETS_INFO[wallet].name }` }>
      <Box className={ className } display="inline-flex" cursor="pointer" onClick={ handleClick } flexShrink={ 0 } aria-label="Add token to wallet">
        <IconSvg name={ WALLETS_INFO[wallet].icon } boxSize={ iconSize }/>
      </Box>
    </Tooltip>
  );
};

export default React.memo(chakra(AddressAddToWallet));
