import React from 'react';
import type { Path, FieldValues } from 'react-hook-form';
import { useController, useFormContext } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';

import type { SelectAsyncProps } from '../../../chakra/select';
import { SelectAsync } from '../../../chakra/select';
import { getFormFieldErrorText } from '../utils/getFormFieldErrorText';

export type FormFieldSelectAsyncProps<
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
> = FormFieldPropsBase<FormFields, Name> & SelectAsyncProps;

const FormFieldSelectAsyncContent = <
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
>(props: FormFieldSelectAsyncProps<FormFields, Name>) => {
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
    <SelectAsync
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

export const FormFieldSelectAsync = React.memo(FormFieldSelectAsyncContent) as typeof FormFieldSelectAsyncContent;
