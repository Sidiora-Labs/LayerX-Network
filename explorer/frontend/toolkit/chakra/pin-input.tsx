import { PinInput as ChakraPinInput, Group } from '@chakra-ui/react';
import * as React from 'react';

export interface PinInputProps extends ChakraPinInput.RootProps {
  rootRef?: React.Ref<HTMLDivElement>;
  count?: number;
  inputProps?: React.InputHTMLAttributes<HTMLInputElement>;
  attached?: boolean;
}

export const PinInput = React.forwardRef<HTMLInputElement, PinInputProps>(
  function PinInput(props, ref) {
    const { count = 6, inputProps, rootRef, attached, placeholder = ' ', bgColor, ...rest } = props;
    return (
      <ChakraPinInput.Root ref={ rootRef } placeholder={ placeholder } { ...rest }>
        <ChakraPinInput.HiddenInput ref={ ref } { ...inputProps }/>
        <ChakraPinInput.Control>
          <Group attached={ attached }>
            { Array.from({ length: count }).map((_, index) => (
              <ChakraPinInput.Input key={ index } index={ index } bgColor={ bgColor }/>
            )) }
          </Group>
        </ChakraPinInput.Control>
      </ChakraPinInput.Root>
    );
  },
);
