import { chakra } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import config from 'configs/app';
import { getResourceKey } from 'lib/api/useApiQuery';
import usePreventFocusAfterModalClosing from 'lib/hooks/usePreventFocusAfterModalClosing';
import * as mixpanel from 'lib/mixpanel/index';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import IconSvg from 'ui/shared/IconSvg';
import AuthGuard from 'ui/snippets/auth/guard/AuthGuard';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';
import WatchlistAddModal from 'ui/watchlist/AddressModal/AddressModal';
import DeleteAddressModal from 'ui/watchlist/DeleteAddressModal';

interface Props {
  className?: string;
  hash: string;
  watchListId: number | null | undefined;
}

const AddressFavoriteButton = ({ className, hash, watchListId }: Props) => {
  const addModalProps = useDisclosure();
  const deleteModalProps = useDisclosure();
  const queryClient = useQueryClient();
  const router = useRouter();
  const onFocusCapture = usePreventFocusAfterModalClosing();
  const profileQuery = useProfileQuery();

  const handleAddToFavorite = React.useCallback(() => {
    watchListId ? deleteModalProps.onOpen() : addModalProps.onOpen();
    !watchListId && mixpanel.logEvent(mixpanel.EventTypes.PAGE_WIDGET, { Type: 'Add to watchlist' });
  }, [ watchListId, deleteModalProps, addModalProps ]);

  const handleAddOrDeleteSuccess = React.useCallback(async() => {
    const queryKey = getResourceKey('general:address', { pathParams: { hash: router.query.hash?.toString() } });
    await queryClient.refetchQueries({ queryKey });
    addModalProps.onClose();
  }, [ addModalProps, queryClient, router.query.hash ]);

  const formData = React.useMemo(() => {
    if (typeof watchListId !== 'number') {
      return { address_hash: hash };
    }

    return {
      address_hash: hash,
      id: watchListId,
    };
  }, [ hash, watchListId ]);

  if (!config.features.account.isEnabled) {
    return null;
  }

  return (
    <>
      <AuthGuard onAuthSuccess={ handleAddToFavorite }>
        { ({ onClick }) => (
          <Tooltip content={ `${ watchListId ? 'Remove address from Watch list' : 'Add address to Watch list' }` } disableOnMobile>
            <IconButton
              className={ className }
              aria-label="edit"
              variant="icon_background"
              size="md"
              selected={ Boolean(watchListId) }
              onClick={ onClick }
              onFocusCapture={ onFocusCapture }
            >
              <IconSvg name={ watchListId ? 'star_filled' : 'star_outline' }/>
            </IconButton>
          </Tooltip>
        ) }
      </AuthGuard>
      <WatchlistAddModal
        { ...addModalProps }
        isAdd
        onSuccess={ handleAddOrDeleteSuccess }
        data={ formData }
        hasEmail={ Boolean(profileQuery.data?.email) }
        showEmailAlert
      />
      { formData.id && (
        <DeleteAddressModal
          { ...deleteModalProps }
          data={ formData }
          onSuccess={ handleAddOrDeleteSuccess }
        />
      ) }
    </>
  );
};

export default chakra(AddressFavoriteButton);
