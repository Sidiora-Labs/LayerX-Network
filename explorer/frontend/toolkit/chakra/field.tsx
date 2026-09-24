import { Field as ChakraField } from '@chakra-ui/react';
import * as React from 'react';

import getComponentDisplayName from '../utils/getComponentDisplayName';
import { space } from '../utils/htmlEntities';
import type { InputProps } from './input';
import type { InputGroupProps } from './input-group';

export interface FieldProps extends Omit<ChakraField.RootProps, 'label' | 'children' | 'size'> {
  label?: React.ReactNode;
  helperText?: React.ReactNode;
  errorText?: React.ReactNode;
  optionalText?: React.ReactNode;
  children: React.ReactElement<InputProps> | React.ReactElement<InputGroupProps>;
  size?: 'sm' | 'md' | 'lg' | '2xl';
}

export const Field = React.forwardRef<HTMLDivElement, FieldProps>(
  function Field(props, ref) {
    const { label, children, helperText, errorText, optionalText, ...rest } = props;

    // A floating field cannot be without a label.
    if (rest.floating && label) {
      const injectedProps = {
        className: 'peer',
        placeholder: ' ',
        size: rest.size,
        floating: rest.floating,
        bgColor: rest.bgColor,
        disabled: rest.disabled,
        readOnly: rest.readOnly,
      };

      const labelElement = (
        <ChakraField.Label bgColor={ rest.bgColor }>
          { label }
          <ChakraField.RequiredIndicator fallback={ optionalText }/>
          { errorText && (
            <ChakraField.ErrorText ml="2px">-{ space }{ errorText }</ChakraField.ErrorText>
          ) }
        </ChakraField.Label>
      );

      const helperTextElement = helperText && (
        <ChakraField.HelperText>{ helperText }</ChakraField.HelperText>
      );

      const child = React.Children.only<React.ReactElement<InputProps | InputGroupProps>>(children);
      const isInputGroup = getComponentDisplayName(child.type) === 'InputGroup';

      if (isInputGroup) {
        const inputElement = React.cloneElement(
          React.Children.only<React.ReactElement<InputProps>>(child.props.children as React.ReactElement<InputProps>),
          injectedProps,
        );

        const groupInputElement = React.cloneElement(child,
          {},
          inputElement,
          labelElement,
        );

        return (
          <ChakraField.Root pos="relative" w="full" ref={ ref } { ...rest }>
            { groupInputElement }
            { helperTextElement }
          </ChakraField.Root>
        );
      }

      const inputElement = React.cloneElement(child, injectedProps);

      return (
        <ChakraField.Root pos="relative" w="full" ref={ ref } { ...rest }>
          { inputElement }
          { labelElement }
          { helperTextElement }
        </ChakraField.Root>
      );
    }

    // Pass size value to the input component
    const injectedProps = {
      size: rest.size,
    };
    const child = React.Children.only<React.ReactElement<InputProps | InputGroupProps>>(children);
    const clonedChild = React.cloneElement(child, injectedProps);

    return (
      <ChakraField.Root ref={ ref } { ...rest }>
        { label && (
          <ChakraField.Label>
            { label }
            <ChakraField.RequiredIndicator fallback={ optionalText }/>
          </ChakraField.Label>
        ) }
        { clonedChild }
        { helperText && (
          <ChakraField.HelperText>{ helperText }</ChakraField.HelperText>
        ) }
        { errorText && (
          <ChakraField.ErrorText>{ errorText }</ChakraField.ErrorText>
        ) }
      </ChakraField.Root>
    );
  },
);
