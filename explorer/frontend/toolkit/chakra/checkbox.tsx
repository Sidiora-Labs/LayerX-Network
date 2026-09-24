import type { Checkbox as ArkCheckbox } from '@ark-ui/react/checkbox';
import type { HTMLChakraProps } from '@chakra-ui/react';
import { Checkbox as ChakraCheckbox, CheckboxGroup as ChakraCheckboxGroup } from '@chakra-ui/react';
import * as React from 'react';

export interface CheckboxProps extends ChakraCheckbox.RootProps {
  icon?: React.ReactNode;
  inputProps?: React.InputHTMLAttributes<HTMLInputElement>;
  rootRef?: React.Ref<HTMLLabelElement>;
}

export const Checkbox = React.forwardRef<HTMLInputElement, CheckboxProps>(
  function Checkbox(props, ref) {
    const { icon, children, inputProps, rootRef, ...rest } = props;
    return (
      <ChakraCheckbox.Root ref={ rootRef } { ...rest }>
        <ChakraCheckbox.HiddenInput ref={ ref } { ...inputProps }/>
        <ChakraCheckbox.Control>
          { icon || <ChakraCheckbox.Indicator/> }
        </ChakraCheckbox.Control>
        { children != null && (
          <ChakraCheckbox.Label>{ children }</ChakraCheckbox.Label>
        ) }
      </ChakraCheckbox.Root>
    );
  },
);

export interface CheckboxGroupProps extends HTMLChakraProps<'div', ArkCheckbox.GroupProps> {
  orientation?: 'vertical' | 'horizontal';
}

export const CheckboxGroup = React.forwardRef<HTMLDivElement, CheckboxGroupProps>(
  function CheckboxGroup(props, ref) {
    const { children, orientation = 'vertical', ...rest } = props;
    return (
      <ChakraCheckboxGroup
        // @ts-ignore: ChakraCheckboxGroup is not typed correctly
        ref={ ref }
        orientation={ orientation }
        display="flex"
        flexDirection={ orientation === 'vertical' ? 'column' : 'row' }
        gap={ orientation === 'vertical' ? 3 : 8 }
        { ...rest }
      >
        { children }
      </ChakraCheckboxGroup>
    );
  },
);
