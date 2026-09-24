import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';
import { useController, useFormContext } from 'react-hook-form';
import { NumericFormat } from 'react-number-format';

import type { ContractAbiItemInput } from '../types';

import { Button } from 'toolkit/chakra/button';
import { Field } from 'toolkit/chakra/field';
import { Input } from 'toolkit/chakra/input';
import { InputGroup } from 'toolkit/chakra/input-group';
import { ClearButton } from 'toolkit/components/buttons/ClearButton';
import { HOUR, SECOND } from 'toolkit/utils/consts';

import ContractMethodAddressButton from './ContractMethodAddressButton';
import ContractMethodFieldLabel from './ContractMethodFieldLabel';
import ContractMethodMultiplyButton from './ContractMethodMultiplyButton';
import useFormatFieldValue from './useFormatFieldValue';
import useValidateField from './useValidateField';
import { matchInt } from './utils';

const TIMESTAMP_BUTTON_REGEXP = /time|deadline|expiration|expiry/i;

interface Props {
  data: ContractAbiItemInput;
  hideLabel?: boolean;
  path: string;
  className?: string;
  isDisabled: boolean;
  isOptional?: boolean;
  level: number;
}

const ContractMethodFieldInput = ({ data, hideLabel, path: name, className, isDisabled, isOptional: isOptionalProp, level }: Props) => {
  const ref = React.useRef<HTMLInputElement>(null);

  const [ intPower, setIntPower ] = React.useState<number>(18);

  const isNativeCoin = data.fieldType === 'native_coin';
  const isOptional = isOptionalProp || isNativeCoin;

  const argTypeMatchInt = React.useMemo(() => matchInt(data.type), [ data.type ]);
  const hasTimestampButton = React.useMemo(() => TIMESTAMP_BUTTON_REGEXP.test(data.name || ''), [ data.name ]);
  const validate = useValidateField({ isOptional, argType: data.type, argTypeMatchInt });
  const format = useFormatFieldValue({ argType: data.type, argTypeMatchInt });

  const { control, setValue, getValues } = useFormContext();
  const { field, fieldState } = useController({ control, name, rules: { validate } });

  const nativeCoinRowBgColor = { _light: 'gray.100', _dark: 'gray.700' };

  const hasMultiplyButton = argTypeMatchInt && Number(argTypeMatchInt.power) >= 64;

  React.useImperativeHandle(field.ref, () => ref.current);

  const handleChange = React.useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    const formattedValue = format(event.target.value);
    field.onChange(formattedValue); // data send back to hook form
    setValue(name, formattedValue); // UI state
  }, [ field, name, setValue, format ]);

  const handleClear = React.useCallback(() => {
    setValue(name, '');
    ref.current?.focus();
  }, [ name, setValue ]);

  const handleMultiplyButtonClick = React.useCallback((power: number) => {
    const zeroes = Array(power).fill('0').join('');
    const value = getValues(name);
    const newValue = format(value ? value + zeroes : '1' + zeroes);
    setValue(name, newValue, { shouldValidate: true });
  }, [ format, getValues, name, setValue ]);

  const handleMaxIntButtonClick = React.useCallback(() => {
    if (!argTypeMatchInt) {
      return;
    }

    const newValue = format(argTypeMatchInt.max.toString());
    setValue(name, newValue, { shouldValidate: true });
  }, [ format, name, setValue, argTypeMatchInt ]);

  const handleAddressButtonClick = React.useCallback((address: string) => {
    const newValue = format(address);
    setValue(name, newValue, { shouldValidate: true });
  }, [ format, name, setValue ]);

  const handleTimestampButtonClick = React.useCallback(() => {
    const newValue = format(String(Math.floor((Date.now() + HOUR) / SECOND)));
    setValue(name, newValue, { shouldValidate: true });
  }, [ format, name, setValue ]);

  const handlePaste = React.useCallback((event: React.ClipboardEvent<HTMLInputElement>) => {
    if (!argTypeMatchInt || !hasMultiplyButton) {
      return;
    }

    const value = Number(event.clipboardData.getData('text'));

    if (Object.is(value, NaN)) {
      return;
    }

    const isFloat = Number.isFinite(value) && !Number.isInteger(value);

    if (!isFloat) {
      return;
    }

    event.preventDefault();

    if (field.value) {
      return;
    }

    const newValue = value * 10 ** intPower;
    const formattedValue = format(newValue.toString());

    field.onChange(formattedValue);
    setValue(name, formattedValue, { shouldValidate: true });
    window.setTimeout(() => {
      // move cursor to the end of the input
      // but we have to wait for the input to get the new value
      const END_OF_INPUT = 100;
      ref.current?.setSelectionRange(END_OF_INPUT, END_OF_INPUT);
    }, 100);
  }, [ argTypeMatchInt, hasMultiplyButton, intPower, format, field, setValue, name ]);

  const error = fieldState.error;

  const inputEndElement = (
    <Flex alignItems="center">
      <ClearButton onClick={ handleClear } disabled={ isDisabled } visible={ field.value !== undefined && field.value !== '' }/>
      { data.type === 'address' && <ContractMethodAddressButton onClick={ handleAddressButtonClick } isDisabled={ isDisabled }/> }
      { argTypeMatchInt && !isNativeCoin && (hasTimestampButton ? (
        <Button
          variant="subtle"
          size="xs"
          textStyle="md"
          fontWeight={ 500 }
          ml={ 1 }
          onClick={ handleTimestampButtonClick }
          disabled={ isDisabled }
        >
          Now+1h
        </Button>
      ) : (
        <Button
          variant="subtle"
          size="xs"
          textStyle="md"
          fontWeight={ 500 }
          ml={ 1 }
          onClick={ handleMaxIntButtonClick }
          disabled={ isDisabled }
        >
          Max
        </Button>
      )) }
      { hasMultiplyButton && (
        <ContractMethodMultiplyButton
          onClick={ handleMultiplyButtonClick }
          isDisabled={ isDisabled }
          initialValue={ intPower }
          onChange={ setIntPower }
        />
      ) }
    </Flex>
  );

  const inputProps = {
    ...field,
    size: 'sm' as const,
    bgColor: 'bg.primary',
    onChange: handleChange,
    required: !isOptional,
    placeholder: data.type,
    autoComplete: 'off',
    'data-1p-ignore': true,
  };

  const getInputRef = React.useCallback((element: HTMLInputElement) => {
    ref.current = element;
  }, []);

  return (
    <Flex
      className={ className }
      flexDir={{ base: 'column', md: 'row' }}
      alignItems="flex-start"
      columnGap={ 3 }
      w="100%"
      bgColor={ isNativeCoin ? nativeCoinRowBgColor : undefined }
      borderRadius="base"
      px="6px"
      py={ isNativeCoin ? 1 : 0 }
    >
      { !hideLabel && <ContractMethodFieldLabel data={ data } isOptional={ isOptional } level={ level }/> }
      <Field invalid={ Boolean(error) } errorText={ error?.message } disabled={ isDisabled }>
        <InputGroup
          endElement={ inputEndElement }
          endElementProps={{ pl: 0, pr: 1 }}
        >
          { argTypeMatchInt ? (
            <Input
              { ...inputProps }
              { ...(error ? { 'data-invalid': true } : {}) }
              onPaste={ handlePaste }
              disabled={ isDisabled }
              asChild
            >
              <NumericFormat
                thousandSeparator=" "
                decimalScale={ 0 }
                allowNegative={ !argTypeMatchInt.isUnsigned }
                getInputRef={ getInputRef }
              />
            </Input>
          ) : <Input { ...inputProps } ref={ ref as React.LegacyRef<HTMLInputElement> | undefined }/> }
        </InputGroup>
      </Field>
    </Flex>
  );
};

export default React.memo(chakra(ContractMethodFieldInput));
