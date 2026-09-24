import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { TokenInfo, TokenInstance } from 'types/api/token';

import { useMultichainContext } from 'lib/contexts/multichain';
import { getTokenTypeName } from 'lib/token/tokenTypes';
import { Link } from 'toolkit/chakra/link';
import { Tag } from 'toolkit/chakra/tag';
import * as regexp from 'toolkit/utils/regexp';
import AddressQrCode from 'ui/address/details/AddressQrCode';
import AccountActionsMenu from 'ui/shared/AccountActionsMenu/AccountActionsMenu';
import AddressAddToWallet from 'ui/shared/address/AddressAddToWallet';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import PageTitle from 'ui/shared/Page/PageTitle';

interface Props {
  isLoading: boolean;
  token: TokenInfo | undefined;
  instance: TokenInstance | undefined;
  hash: string | undefined;
}

const TokenInstancePageTitle = ({ isLoading, token, instance, hash }: Props) => {
  const multichainContext = useMultichainContext();

  const title = (() => {
    if (typeof instance?.metadata?.name === 'string') {
      return instance.metadata.name;
    }

    if (!instance) {
      return `Unknown token instance`;
    }

    if (token?.name || token?.symbol) {
      return (token.name || token.symbol) + ' #' + instance.id;
    }

    return `ID ${ instance.id }`;
  })();

  const tokenTag = token ? <Tag loading={ isLoading }>{ getTokenTypeName(token.type) }</Tag> : null;

  const appLink = (() => {
    if (!instance?.external_app_url) {
      return null;
    }

    try {
      const url = regexp.URL_PREFIX.test(instance.external_app_url) ?
        new URL(instance.external_app_url) :
        new URL('https://' + instance.external_app_url);

      return (
        <Link external href={ url.toString() } variant="underlaid" loading={ isLoading } ml={{ base: 0, lg: 'auto' }}>
          { url.hostname || instance.external_app_url }
        </Link>
      );
    } catch (error) {
      return (
        <Link external href={ instance.external_app_url } variant="underlaid" loading={ isLoading } ml={{ base: 0, lg: 'auto' }}>
          View in app
        </Link>
      );
    }
  })();

  const address = {
    hash: hash || '',
    is_contract: true,
    implementations: null,
    watchlist_names: [],
    watchlist_address_id: null,
  };

  const titleSecondRow = (
    <Flex alignItems="center" w="100%" minW={ 0 } columnGap={ 2 } rowGap={ 2 } flexWrap={{ base: 'wrap', lg: 'nowrap' }}>
      { token && (
        <TokenEntity
          token={ token }
          isLoading={ isLoading }
          noSymbol
          noCopy
          jointSymbol
          variant="subheading"
          w="auto"
          maxW="700px"
          chain={ multichainContext?.chain }
        />
      ) }
      { !isLoading && token && <AddressAddToWallet token={ token } tokenId={ instance?.id } variant="button"/> }
      <AddressQrCode hash={ address.hash } isLoading={ isLoading }/>
      <AccountActionsMenu isLoading={ isLoading } showUpdateMetadataItem/>
      { appLink }
    </Flex>
  );

  return (
    <PageTitle
      title={ title }
      contentAfter={ tokenTag }
      secondRow={ titleSecondRow }
      isLoading={ isLoading }
    />
  );
};

export default React.memo(TokenInstancePageTitle);
