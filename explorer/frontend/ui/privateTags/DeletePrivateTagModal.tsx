import { Text } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React, { useCallback } from 'react';

import type { AddressTag, TransactionTag, AddressTagsResponse, TransactionTagsResponse } from 'types/api/account';

import useApiFetch from 'lib/api/useApiFetch';
import { getResourceKey } from 'lib/api/useApiQuery';
import DeleteModal from 'ui/shared/DeleteModal';

type Props = {
  open: boolean;
  onOpenChange: ({ open }: { open: boolean }) => void;
  data: AddressTag | TransactionTag;
  type: 'address' | 'transaction';
};

const DeletePrivateTagModal: React.FC<Props> = ({ open, onOpenChange, data, type }) => {
  const tag = data.name;
  const id = data.id;

  const queryClient = useQueryClient();
  const apiFetch = useApiFetch();

  const mutationFn = useCallback(() => {
    const resourceName = type === 'address' ? 'general:private_tags_address' : 'general:private_tags_tx';
    return apiFetch(resourceName, {
      pathParams: { id: String(data.id) },
      fetchParams: { method: 'DELETE' },
    });
  }, [ type, apiFetch, data.id ]);

  const onSuccess = useCallback(async() => {
    if (type === 'address') {
      queryClient.setQueryData(getResourceKey('general:private_tags_address'), (prevData: AddressTagsResponse | undefined) => {
        const newItems = prevData?.items.filter((item: AddressTag) => item.id !== id);
        return { ...prevData, items: newItems };

      });
    } else {
      queryClient.setQueryData(getResourceKey('general:private_tags_tx'), (prevData: TransactionTagsResponse | undefined) => {
        const newItems = prevData?.items.filter((item: TransactionTag) => item.id !== id);
        return { ...prevData, items: newItems };
      });
    }
  }, [ type, id, queryClient ]);

  const renderText = useCallback(() => {
    return (
      <Text>Tag<Text fontWeight="700" as="span">{ ` "${ tag || 'tag' }" ` }</Text>will be deleted</Text>
    );
  }, [ tag ]);

  return (
    <DeleteModal
      open={ open }
      onOpenChange={ onOpenChange }
      title="Removal of private tag"
      renderContent={ renderText }
      mutationFn={ mutationFn }
      onSuccess={ onSuccess }
    />
  );
};

export default DeletePrivateTagModal;
