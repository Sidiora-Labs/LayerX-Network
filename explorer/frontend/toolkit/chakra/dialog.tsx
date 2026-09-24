import { Dialog as ChakraDialog, Portal } from '@chakra-ui/react';
import * as React from 'react';

import { BackToButton } from '../components/buttons/BackToButton';
import { CloseButton } from './close-button';

interface DialogContentProps extends ChakraDialog.ContentProps {
  portalled?: boolean;
  portalRef?: React.RefObject<HTMLElement>;
  backdrop?: boolean;
}

export const DialogContent = React.forwardRef<
  HTMLDivElement,
  DialogContentProps
>(function DialogContent(props, ref) {
  const {
    children,
    portalled = true,
    portalRef,
    backdrop = true,
    ...rest
  } = props;

  return (
    <Portal disabled={ !portalled } container={ portalRef }>
      { backdrop && <ChakraDialog.Backdrop/> }
      <ChakraDialog.Positioner>
        <ChakraDialog.Content ref={ ref } { ...rest } asChild={ false }>
          { children }
        </ChakraDialog.Content>
      </ChakraDialog.Positioner>
    </Portal>
  );
});

export const DialogCloseTrigger = React.forwardRef<
  HTMLButtonElement,
  ChakraDialog.CloseTriggerProps
>(function DialogCloseTrigger(props, ref) {
  return (
    <ChakraDialog.CloseTrigger
      { ...props }
      asChild
    >
      <CloseButton ref={ ref }>
        { props.children }
      </CloseButton>
    </ChakraDialog.CloseTrigger>
  );
});

export interface DialogHeaderProps extends ChakraDialog.HeaderProps {
  startElement?: React.ReactNode;
  onBackToClick?: () => void;
}

export const DialogHeader = React.forwardRef<
  HTMLDivElement,
  DialogHeaderProps
>(function DialogHeader(props, ref) {
  const { startElement: startElementProp, onBackToClick, ...rest } = props;

  const startElement = startElementProp ?? (onBackToClick && <BackToButton onClick={ onBackToClick }/>);

  return (
    <ChakraDialog.Header ref={ ref } { ...rest }>
      { startElement }
      <ChakraDialog.Title>{ props.children }</ChakraDialog.Title>
      <DialogCloseTrigger ml="auto"/>
    </ChakraDialog.Header>
  );
});

export const DialogRoot = ChakraDialog.Root;
export const DialogFooter = ChakraDialog.Footer;
export const DialogBody = ChakraDialog.Body;
export const DialogBackdrop = ChakraDialog.Backdrop;
export const DialogTitle = ChakraDialog.Title;
export const DialogDescription = ChakraDialog.Description;
export const DialogTrigger = ChakraDialog.Trigger;
export const DialogActionTrigger = ChakraDialog.ActionTrigger;
