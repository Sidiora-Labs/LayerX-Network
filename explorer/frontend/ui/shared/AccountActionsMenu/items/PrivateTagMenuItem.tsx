import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { ItemProps } from '../types';
import type { Address } from 'types/api/address';
import type { Transaction } from 'types/api/transaction';

import { getResourceKey } from 'lib/api/useApiQuery';
import getPageType from 'lib/mixpanel/getPageType';
import { MenuItem } from 'toolkit/chakra/menu';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AddressModal from 'ui/privateTags/AddressModal/AddressModal';
import TransactionModal from 'ui/privateTags/TransactionModal/TransactionModal';
import IconSvg from 'ui/shared/IconSvg';
import AuthGuard from 'ui/snippets/auth/guard/AuthGuard';

import ButtonItem from '../parts/ButtonItem';

interface Props extends ItemProps {
  entityType?: 'address' | 'tx';
}

const PrivateTagMenuItem = ({ hash, entityType = 'address', type }: Props) => {
  const modal = useDisclosure();
  const queryClient = useQueryClient();
  const router = useRouter();

  const queryKey = getResourceKey(entityType === 'tx' ? 'general:tx' : 'general:address', { pathParams: { hash } });
  const queryData = queryClient.getQueryData<Address | Transaction>(queryKey);

  const handleAddPrivateTag = React.useCallback(async() => {
    await queryClient.refetchQueries({ queryKey });
    modal.onClose();
  }, [ queryClient, queryKey, modal ]);

  if (
    queryData &&
    (
      ('private_tags' in queryData && queryData.private_tags?.length) ||
      ('transaction_tag' in queryData && queryData.transaction_tag)
    )
  ) {
    return null;
  }

  const pageType = getPageType(router.pathname);
  const modalProps = {
    open: modal.open,
    onOpenChange: modal.onOpenChange,
    onSuccess: handleAddPrivateTag,
    pageType,
  };

  const element = (() => {
    switch (type) {
      case 'button': {
        return (
          <AuthGuard onAuthSuccess={ modal.onOpen }>
            { ({ onClick }) => (
              // FIXME use non-navigation icon
              <ButtonItem label="Add private tag" icon="navigation/private_tags" onClick={ onClick }/>
            ) }
          </AuthGuard>
        );
      }
      case 'menu_item': {
        return (
          <AuthGuard onAuthSuccess={ modal.onOpen }>
            { ({ onClick }) => (
              <MenuItem onClick={ onClick } value="add-private-tag">
                { /* FIXME use non-navigation icon */ }
                <IconSvg name="navigation/private_tags" boxSize={ 6 }/>
                <span>Add private tag</span>
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
      { entityType === 'tx' ?
        <TransactionModal { ...modalProps } data={{ transaction_hash: hash }}/> :
        <AddressModal { ...modalProps } data={{ address_hash: hash }}/>
      }
    </>
  );
};

export default React.memo(PrivateTagMenuItem);
