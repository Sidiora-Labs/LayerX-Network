import { Box, Text } from '@chakra-ui/react';
import React, { useCallback, useState } from 'react';

import type { ApiKey } from 'types/api/account';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { API_KEY } from 'stubs/account';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import { space } from 'toolkit/utils/htmlEntities';
import ApiKeyModal from 'ui/apiKey/ApiKeyModal/ApiKeyModal';
import ApiKeyListItem from 'ui/apiKey/ApiKeyTable/ApiKeyListItem';
import ApiKeyTable from 'ui/apiKey/ApiKeyTable/ApiKeyTable';
import DeleteApiKeyModal from 'ui/apiKey/DeleteApiKeyModal';
import AccountPageDescription from 'ui/shared/AccountPageDescription';
import AlertWithExternalHtml from 'ui/shared/alerts/AlertWithExternalHtml';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import PageTitle from 'ui/shared/Page/PageTitle';
import useRedirectForInvalidAuthToken from 'ui/snippets/auth/useRedirectForInvalidAuthToken';

const DATA_LIMIT = 3;

const apiKeysAlertHtml = config.UI.apiKeysAlert.message;
const feature = config.features.account;

const ApiKeysPage: React.FC = () => {
  const apiKeyModalProps = useDisclosure();
  const deleteModalProps = useDisclosure();
  useRedirectForInvalidAuthToken();

  const [ apiKeyModalData, setApiKeyModalData ] = useState<ApiKey>();
  const [ deleteModalData, setDeleteModalData ] = useState<ApiKey>();

  const { data, isPlaceholderData, isError } = useApiQuery('general:api_keys', {
    queryOptions: {
      placeholderData: Array(3).fill(API_KEY),
    },
  });

  const onEditClick = useCallback((data: ApiKey) => {
    setApiKeyModalData(data);
    apiKeyModalProps.onOpen();
  }, [ apiKeyModalProps ]);

  const onApiKeyModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setApiKeyModalData(undefined);
    apiKeyModalProps.onOpenChange({ open });
  }, [ apiKeyModalProps ]);

  const onDeleteClick = useCallback((data: ApiKey) => {
    setDeleteModalData(data);
    deleteModalProps.onOpen();
  }, [ deleteModalProps ]);

  const onDeleteModalOpenChange = useCallback(({ open }: { open: boolean }) => {
    !open && setDeleteModalData(undefined);
    deleteModalProps.onOpenChange({ open });
  }, [ deleteModalProps ]);

  const description = (
    <AccountPageDescription>
      Create API keys to use for your RPC and EthRPC API requests. For more information, see { space }
      <Link href="https://docs.blockscout.com/using-blockscout/my-account/api-keys#api-keys" external noIcon>"How to use a Blockscout API key"</Link>.
    </AccountPageDescription>
  );

  const content = (() => {
    if (isError) {
      return <DataFetchAlert/>;
    }

    const list = (
      <>
        <Box display={{ base: 'block', lg: 'none' }}>
          { data?.map((item, index) => (
            <ApiKeyListItem
              key={ item.api_key + (isPlaceholderData ? index : '') }
              item={ item }
              isLoading={ isPlaceholderData }
              onDeleteClick={ onDeleteClick }
              onEditClick={ onEditClick }
            />
          )) }
        </Box>
        <Box display={{ base: 'none', lg: 'block' }}>
          <ApiKeyTable
            data={ data }
            isLoading={ isPlaceholderData }
            onDeleteClick={ onDeleteClick }
            onEditClick={ onEditClick }
            limit={ DATA_LIMIT }
          />
        </Box>
      </>
    );

    const canAdd = !isPlaceholderData ? (data?.length || 0) < DATA_LIMIT : true;

    const alert = apiKeysAlertHtml ? <AlertWithExternalHtml html={ apiKeysAlertHtml } status="warning" mb={ 6 }/> : null;

    const button = (() => {
      if (!feature.isEnabled || feature.apiKeysButton === false) {
        return null;
      }

      if (typeof feature.apiKeysButton === 'string') {
        return (
          <Link href={ feature.apiKeysButton } external noIcon>
            <Button>Add API key</Button>
          </Link>
        );
      }

      return (
        <Button
          onClick={ apiKeyModalProps.onOpen }
          disabled={ !canAdd }
        >
          Add API key
        </Button>
      );
    })();

    return (
      <>
        { description }
        { alert }
        { Boolean(data?.length) && list }
        <Skeleton
          marginTop={ 8 }
          flexDir={{ base: 'column', lg: 'row' }}
          alignItems={{ base: 'start', lg: 'center' }}
          loading={ isPlaceholderData }
          display="inline-flex"
          columnGap={ 5 }
          rowGap={ 5 }
        >
          { button }
          { !canAdd && (
            <Text fontSize="sm" color="text.secondary">
              { `You have added the maximum number of API keys (${ DATA_LIMIT }). Contact us to request additional keys.` }
            </Text>
          ) }
        </Skeleton>
        <ApiKeyModal open={ apiKeyModalProps.open } onOpenChange={ onApiKeyModalOpenChange } data={ apiKeyModalData }/>
        { deleteModalData && <DeleteApiKeyModal open={ deleteModalProps.open } onOpenChange={ onDeleteModalOpenChange } data={ deleteModalData }/> }
      </>
    );
  })();

  return (
    <>
      <PageTitle title="API keys"/>
      { content }
    </>
  );
};

export default ApiKeysPage;
