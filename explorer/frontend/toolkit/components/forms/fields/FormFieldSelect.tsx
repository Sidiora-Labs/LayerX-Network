import React from 'react';
import type { Path, FieldValues } from 'react-hook-form';
import { useController, useFormContext } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';

import type { SelectProps } from '../../../chakra/select';
import { Select } from '../../../chakra/select';
import { getFormFieldErrorText } from '../utils/getFormFieldErrorText';

export type FormFieldSelectProps<
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
> = FormFieldPropsBase<FormFields, Name> & SelectProps;

const FormFieldSelectContent = <
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
>(props: FormFieldSelectProps<FormFields, Name>) => {
  const { name, rules, size = 'lg', controllerProps, ...rest } = props;

  const { control } = useFormContext<FormFields>();
  const { field, fieldState, formState } = useController<FormFields, typeof name>({
    control,
    name,
    rules: { ...rules, required: rest.required },
    ...controllerProps,
  });

  const isDisabled = formState.isSubmitting;

  const handleChange = React.useCallback(({ value }: { value: Array<string> }) => {
    field.onChange(value);
  }, [ field ]);

  const handleBlur = React.useCallback(() => {
    field.onBlur();
  }, [ field ]);

  return (
    <Select
      ref={ field.ref }
      name={ field.name }
      value={ field.value }
      onBlur={ field.onBlur }
      onValueChange={ handleChange }
      onInteractOutside={ handleBlur }
      disabled={ isDisabled }
      invalid={ Boolean(fieldState.error) }
      errorText={ getFormFieldErrorText(fieldState.error) }
      size={ size }
      positioning={{ sameWidth: true }}
      { ...rest }
    />
  );
};

export const FormFieldSelect = React.memo(FormFieldSelectContent) as typeof FormFieldSelectContent;
