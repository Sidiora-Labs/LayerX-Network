import type { AlertDescriptionProps } from '@chakra-ui/react';
import { Alert as ChakraAlert, Icon } from '@chakra-ui/react';
import * as React from 'react';

import IndicatorIcon from 'icons/info_filled.svg';

import { CloseButton } from './close-button';
import { Skeleton } from './skeleton';

export interface AlertProps extends Omit<ChakraAlert.RootProps, 'title'> {
  startElement?: React.ReactNode;
  endElement?: React.ReactNode;
  descriptionProps?: AlertDescriptionProps;
  title?: React.ReactNode;
  icon?: React.ReactElement;
  closable?: boolean;
  onClose?: () => void;
  loading?: boolean;
  showIcon?: boolean;
}

export const Alert = React.forwardRef<HTMLDivElement, AlertProps>(
  function Alert(props, ref) {
    const {
      title,
      children,
      icon,
      closable,
      onClose,
      startElement,
      endElement,
      loading,
      size,
      showIcon = false,
      descriptionProps,
      ...rest
    } = props;

    const [ isOpen, setIsOpen ] = React.useState(true);

    const defaultIcon = <Icon boxSize={ 5 }><IndicatorIcon/></Icon>;

    const iconElement = (() => {
      if (startElement !== undefined) {
        return startElement;
      }

      if (!showIcon && icon === undefined) {
        return null;
      }

      return <ChakraAlert.Indicator>{ icon || defaultIcon }</ChakraAlert.Indicator>;
    })();

    const handleClose = React.useCallback(() => {
      setIsOpen(false);
      onClose?.();
    }, [ onClose ]);

    if (closable && !isOpen) {
      return null;
    }

    return (
      <Skeleton loading={ loading } asChild>
        <ChakraAlert.Root ref={ ref } size={ size ?? { base: 'sm', lg: 'md' } } { ...rest }>
          { iconElement }
          { children ? (
            <ChakraAlert.Content>
              { title && <ChakraAlert.Title>{ title }</ChakraAlert.Title> }
              <ChakraAlert.Description display="inline-flex" flexWrap="wrap" { ...descriptionProps }>{ children }</ChakraAlert.Description>
            </ChakraAlert.Content>
          ) : (
            <ChakraAlert.Title flex="1">{ title }</ChakraAlert.Title>
          ) }
          { endElement }
          { closable && (
            <CloseButton
              pos="relative"
              // default size for mobile is "sm", and for desktop is "md"
              m={{ base: size === 'md' ? 0.5 : 0, lg: size === 'sm' ? 0 : 0.5 }}
              alignSelf="flex-start"
              onClick={ handleClose }
            />
          ) }
        </ChakraAlert.Root>
      </Skeleton>
    );
  },
);
