import { chakra } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { ItemProps } from '../types';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { PAGE_TYPE_DICT } from 'lib/mixpanel/getPageType';
import { MenuItem } from 'toolkit/chakra/menu';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AddressVerificationModal from 'ui/addressVerification/AddressVerificationModal';
import IconSvg from 'ui/shared/IconSvg';
import AuthGuard from 'ui/snippets/auth/guard/AuthGuard';
import useIsAuth from 'ui/snippets/auth/useIsAuth';

import ButtonItem from '../parts/ButtonItem';

const TokenInfoMenuItem = ({ hash, type }: ItemProps) => {
  const router = useRouter();
  const modal = useDisclosure();
  const isAuth = useIsAuth();

  const verifiedAddressesQuery = useApiQuery('contractInfo:verified_addresses', {
    pathParams: { chainId: config.chain.id },
    queryOptions: {
      enabled: isAuth,
    },
  });
  const applicationsQuery = useApiQuery('admin:token_info_applications', {
    pathParams: { chainId: config.chain.id, id: undefined },
    queryOptions: {
      enabled: isAuth,
    },
  });
  const tokenInfoQuery = useApiQuery('contractInfo:token_verified_info', {
    pathParams: { hash, chainId: config.chain.id },
    queryOptions: {
      refetchOnMount: false,
    },
  });

  const handleAddApplicationClick = React.useCallback(async() => {
    router.push({ pathname: '/account/verified-addresses', query: { address: hash } });
  }, [ hash, router ]);

  const handleVerifiedAddressSubmit = React.useCallback(async() => {
    await verifiedAddressesQuery.refetch();
  }, [ verifiedAddressesQuery ]);

  const handleShowMyAddressesClick = React.useCallback(async() => {
    router.push({ pathname: '/account/verified-addresses' });
  }, [ router ]);

  const element = (() => {
    const isVerifiedAddress = verifiedAddressesQuery.data?.verifiedAddresses
      .find(({ contractAddress }) => contractAddress.toLowerCase() === hash.toLowerCase());
    const hasApplication = applicationsQuery.data?.submissions.some(({ tokenAddress }) => tokenAddress.toLowerCase() === hash.toLowerCase());

    const label = (() => {
      if (!isVerifiedAddress) {
        return tokenInfoQuery.data?.tokenAddress ? 'Update token info' : 'Add token info';
      }

      return hasApplication || tokenInfoQuery.data?.tokenAddress ? 'Update token info' : 'Add token info';
    })();

    const onAuthSuccess = isVerifiedAddress ? handleAddApplicationClick : modal.onOpen;

    switch (type) {
      case 'button': {
        const icon = <IconSvg name="edit" boxSize={ 6 } p={ 0.5 }/>;

        return (
          <AuthGuard onAuthSuccess={ onAuthSuccess } ensureEmail>
            { ({ onClick }) => (
              <ButtonItem label={ label } icon={ icon } onClick={ onClick }/>
            ) }
          </AuthGuard>
        );
      }
      case 'menu_item': {
        const icon = <IconSvg name="edit" boxSize={ 6 } p={ 1 }/>;

        return (
          <AuthGuard onAuthSuccess={ onAuthSuccess } ensureEmail>
            { ({ onClick }) => (
              <MenuItem onClick={ onClick } value="add-token-info">
                { icon }
                <chakra.span>{ label }</chakra.span>
              </MenuItem>
            ) }
          </AuthGuard>
        );
      }
    }
  })();

  return (
    <>
      { element }
      <AddressVerificationModal
        defaultAddress={ hash }
        pageType={ PAGE_TYPE_DICT['/token/[hash]'] }
        open={ modal.open }
        onOpenChange={ modal.onOpenChange }
        onSubmit={ handleVerifiedAddressSubmit }
        onAddTokenInfoClick={ handleAddApplicationClick }
        onShowListClick={ handleShowMyAddressesClick }
      />
    </>
  );
};

export default React.memo(TokenInfoMenuItem);
