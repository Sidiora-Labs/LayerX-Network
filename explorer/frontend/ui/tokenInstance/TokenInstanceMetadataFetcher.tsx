import { Spinner, Center } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import React from 'react';

import type { SocketMessage } from 'lib/socket/types';
import type { TokenInstance } from 'types/api/token';

import config from 'configs/app';
import useApiFetch from 'lib/api/useApiFetch';
import { getResourceKey } from 'lib/api/useApiQuery';
import getErrorMessage from 'lib/errors/getErrorMessage';
import useSocketChannel from 'lib/socket/useSocketChannel';
import useSocketMessage from 'lib/socket/useSocketMessage';
import { Alert } from 'toolkit/chakra/alert';
import { DialogBody, DialogContent, DialogHeader, DialogRoot } from 'toolkit/chakra/dialog';
import { toaster } from 'toolkit/chakra/toaster';
import { MINUTE, SECOND } from 'toolkit/utils/consts';
import ReCaptcha from 'ui/shared/reCaptcha/ReCaptcha';
import useReCaptcha from 'ui/shared/reCaptcha/useReCaptcha';

import { useMetadataUpdateContext } from './contexts/metadataUpdate';

const TOAST_ID = 'token-instance-metadata-fetcher';

interface Props {
  hash: string;
  id: string;
}

const TokenInstanceMetadataFetcher = ({ hash, id }: Props) => {
  const timeoutId = React.useRef<number>(undefined);

  const { status, setStatus } = useMetadataUpdateContext() || {};
  const apiFetch = useApiFetch();
  const queryClient = useQueryClient();
  const recaptcha = useReCaptcha();

  const handleRefreshError = React.useCallback(() => {
    setStatus?.('ERROR');
    toaster.update(TOAST_ID, {
      title: 'Error',
      description: 'The refreshing process has failed. Please try again.',
      type: 'error',
      duration: 5 * SECOND,
    });
  }, [ setStatus ]);

  const apiFetchFactory = React.useCallback(async(recaptchaToken?: string) => {
    return apiFetch<'general:token_instance_refresh_metadata', unknown, unknown>('general:token_instance_refresh_metadata', {
      pathParams: { hash, id },
      fetchParams: {
        method: 'PATCH',
        body: { recaptcha_response: recaptchaToken },
        headers: {
          ...(recaptchaToken && { 'recaptcha-v2-response': recaptchaToken }),
        },
      },
    });
  }, [ apiFetch, hash, id ]);

  const initializeUpdate = React.useCallback(async() => {
    try {
      await recaptcha.fetchProtectedResource(apiFetchFactory);
      setStatus?.('WAITING_FOR_RESPONSE');
      toaster.loading({
        id: TOAST_ID,
        title: 'Please wait',
        description: 'Refetching metadata request sent',
        duration: Infinity,
      });
      timeoutId.current = window.setTimeout(handleRefreshError, 2 * MINUTE);
    } catch (error) {
      toaster.error({
        id: TOAST_ID,
        title: 'Error',
        description: getErrorMessage(error) || 'Unable to initialize metadata update',
      });
      setStatus?.('ERROR');
    }

  }, [ apiFetchFactory, handleRefreshError, recaptcha, setStatus ]);

  const handleModalClose = React.useCallback(({ open }: { open: boolean }) => {
    if (!open) {
      setStatus?.('INITIAL');
    }
  }, [ setStatus ]);

  const handleSocketMessage: SocketMessage.TokenInstanceMetadataFetched['handler'] = React.useCallback((payload) => {
    if (String(payload.token_id) !== id) {
      return;
    }

    const queryKey = getResourceKey('general:token_instance', { queryParams: { hash, id } });
    queryClient.setQueryData(queryKey, (prevData: TokenInstance | undefined): TokenInstance | undefined => {
      if (!prevData) {
        return;
      }

      const castToString = (value: unknown) => typeof value === 'string' ? value : undefined;

      return {
        ...prevData,
        external_app_url: castToString(payload.fetched_metadata?.external_url) ?? null,
        animation_url: castToString(payload.fetched_metadata?.animation_url) ?? null,
        image_url: castToString(
          payload.fetched_metadata?.image ||
            payload.fetched_metadata?.image_url ||
            payload.fetched_metadata?.animation_url,
        ) ?? null,
        metadata: payload.fetched_metadata,
      };
    });

    toaster.update(TOAST_ID, {
      title: 'Success!',
      description: 'Metadata has been refreshed',
      type: 'success',
      duration: 5 * SECOND,
    });

    setStatus?.('SUCCESS');

    window.clearTimeout(timeoutId.current);
  }, [ hash, id, queryClient, setStatus ]);

  const channel = useSocketChannel({
    topic: `token_instances:${ hash.toLowerCase() }`,
    onSocketClose: handleRefreshError,
    onSocketError: handleRefreshError,
    isDisabled: status !== 'WAITING_FOR_RESPONSE',
  });

  useSocketMessage({
    channel,
    event: 'fetched_token_instance_metadata',
    handler: handleSocketMessage,
  });

  React.useEffect(() => {
    if (status !== 'MODAL_OPENED') {
      return;
    }

    const timeoutId = window.setTimeout(initializeUpdate, 100);
    return () => window.clearTimeout(timeoutId);
  }, [ status, initializeUpdate ]);

  React.useEffect(() => {
    return () => {
      timeoutId.current && window.clearTimeout(timeoutId.current);
      toaster.remove(TOAST_ID);
    };
    // run only on mount/unmount
  }, []);

  if (status !== 'MODAL_OPENED') {
    return null;
  }

  return (
    <DialogRoot
      open={ status === 'MODAL_OPENED' }
      onOpenChange={ handleModalClose }
      size={{ lgDown: 'full', lg: 'sm' }}
      trapFocus={ false }
      preventScroll={ false }
      modal={ false }
      closeOnInteractOutside={ false }
    >
      <DialogContent>
        <DialogHeader mb={ 4 }>Sending request</DialogHeader>
        <DialogBody mb={ 0 } minH="78px">
          { config.services.reCaptchaV2.siteKey ? (
            <>
              <Center h="80px">
                <Spinner size="lg"/>
              </Center>
              <ReCaptcha { ...recaptcha } hideWarning/>
            </>
          ) : (
            <Alert status="error">
              Metadata refresh is not available at the moment since reCaptcha is not configured for this application.
              Please contact the service maintainer to make necessary changes in the service configuration.
            </Alert>
          ) }
        </DialogBody>
      </DialogContent>
    </DialogRoot>
  );
};

export default React.memo(TokenInstanceMetadataFetcher);
