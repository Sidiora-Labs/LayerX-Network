import { HStack, Text } from '@chakra-ui/react';
import React from 'react';
import { useController, useFormContext } from 'react-hook-form';

import type { OtpCodeFormFields } from '../types';

import { PinInput } from 'toolkit/chakra/pin-input';

const CODE_LENGTH = 6;

interface Props {
  isDisabled?: boolean;
}

const AuthModalFieldOtpCode = ({ isDisabled: isDisabledProp }: Props) => {
  const { control } = useFormContext<OtpCodeFormFields>();
  const { field, fieldState, formState } = useController<OtpCodeFormFields, 'code'>({
    control,
    name: 'code',
    rules: { required: true, minLength: CODE_LENGTH, maxLength: CODE_LENGTH },
  });

  const isDisabled = isDisabledProp || formState.isSubmitting;

  const handleChange = React.useCallback(({ value }: { value: Array<string> }) => {
    field.onChange(value);
  }, [ field ]);

  return (
    <>
      <HStack>
        <PinInput
          otp
          name={ field.name }
          value={ field.value }
          onValueChange={ handleChange }
          disabled={ isDisabled }
          invalid={ Boolean(fieldState.error) }
          bgColor="dialog.bg"
        />
      </HStack>
      { fieldState.error?.message && <Text color="text.error" textStyle="sm" mt={ 1 }>{ fieldState.error.message }</Text> }
    </>
  );
};

export default React.memo(AuthModalFieldOtpCode);
