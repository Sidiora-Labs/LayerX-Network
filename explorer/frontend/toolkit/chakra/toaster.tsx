'use client';

import {
  Toaster as ChakraToaster,
  Portal,
  Spinner,
  Stack,
  Toast,
  createToaster,
} from '@chakra-ui/react';

import { SECOND } from '../utils/consts';
import { CloseButton } from './close-button';

export const toaster = createToaster({
  placement: 'top-end',
  pauseOnPageIdle: true,
  duration: 10 * SECOND,
  offsets: {
    top: '12px',
    right: '12px',
    bottom: '12px',
    left: '12px',
  },
});

export const Toaster = () => {
  return (
    <Portal>
      <ChakraToaster toaster={ toaster } insetInline={{ mdDown: '4' }}>
        { (toast) => {
          const closable = toast.meta?.closable !== undefined ? toast.meta.closable : true;

          return (
            <Toast.Root width={{ md: 'sm' }}>
              { toast.type === 'loading' ? (
                <Spinner size="sm" color="blue.solid" my={ 1 }/>
              ) : null }
              <Stack gap="0" flex="1" maxWidth="100%">
                { toast.title && <Toast.Title>{ toast.title }</Toast.Title> }
                { (toast.meta?.renderDescription || toast.description) && (
                  <Toast.Description>
                    { toast.meta?.renderDescription?.() || toast.description }
                  </Toast.Description>
                ) }
              </Stack>
              { toast.action && (
                <Toast.ActionTrigger>{ toast.action.label }</Toast.ActionTrigger>
              ) }
              { closable && (
                <Toast.CloseTrigger asChild>
                  <CloseButton/>
                </Toast.CloseTrigger>
              ) }
            </Toast.Root>
          );
        } }
      </ChakraToaster>
    </Portal>
  );
};
