import { Text, chakra } from '@chakra-ui/react';
import React from 'react';

import type { AddressMetadataTagFormatted } from 'types/client/addressMetadata';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import * as mixpanel from 'lib/mixpanel/index';
import { Image } from 'toolkit/chakra/image';
import { Link } from 'toolkit/chakra/link';

type Props = {
  data: NonNullable<AddressMetadataTagFormatted['meta']>;
  className?: string;
  txHash?: string;
  source: 'Txn' | 'NFT collection' | 'NFT item';
};

const AppActionButton = ({ data, className, txHash, source }: Props) => {
  const { appID, textColor, bgColor, appActionButtonText, appLogoURL, appMarketplaceURL } = data;

  const actionURL = appMarketplaceURL?.replace('{chainId}', config.chain.id || '').replace('{txHash}', txHash || '');

  const handleClick = React.useCallback(() => {
    const info = appID || actionURL;
    if (info) {
      mixpanel.logEvent(mixpanel.EventTypes.PAGE_WIDGET, { Type: 'Action button', Info: info, Source: source });
    }
  }, [ source, appID, actionURL ]);

  if ((!appID && !actionURL) || (!appActionButtonText && !appLogoURL)) {
    return null;
  }

  const content = (
    <>
      { appLogoURL && (
        <Image
          src={ appLogoURL }
          alt={ `${ appActionButtonText } button` }
          boxSize={ 5 }
          borderRadius="sm"
          mr={ 2 }
        />
      ) }
      <Text textStyle="sm" fontWeight="500" color="currentColor">
        { appActionButtonText }
      </Text>
    </>
  );

  return (
    <Link
      className={ className }
      href={ actionURL ?? (appID ? route({ pathname: '/apps/[id]', query: { id: appID, action: 'connect' } }) : undefined) }
      external={ Boolean(actionURL) }
      onClick={ handleClick }
      variant="underlaid"
      iconColor={ textColor }
      color={ textColor }
      bg={ bgColor }
      _hover={{ color: textColor, opacity: textColor || bgColor ? 0.9 : 1 }}
      _active={{ color: textColor, opacity: textColor || bgColor ? 0.9 : 1 }}
    >
      { content }
    </Link>
  );
};

export default chakra(AppActionButton);
