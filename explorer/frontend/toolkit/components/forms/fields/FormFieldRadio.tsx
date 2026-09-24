import React from 'react';
import type { FieldValues, Path } from 'react-hook-form';
import { useController, useFormContext } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';
import type { ExcludeUndefined } from 'types/utils';

import type { RadioGroupProps, RadioProps } from '../../../chakra/radio';
import { Radio, RadioGroup } from '../../../chakra/radio';

export interface FormFieldRadioProps<
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
> extends Pick<FormFieldPropsBase<FormFields>, 'rules' | 'controllerProps'>,
  RadioGroupProps {
  name: Name;
  options: Array<{ value: ExcludeUndefined<FormFields[Name]>; label: string }>;
  itemProps?: RadioProps;
}

const FormFieldRadioContent = <
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
>({
  name,
  options,
  itemProps,
  onValueChange,
  disabled,
  controllerProps,
  ...rest
}: FormFieldRadioProps<FormFields, Name>) => {
  const { control } = useFormContext<FormFields>();
  const { field, formState } = useController<FormFields, typeof name>({
    control,
    name,
    ...controllerProps,
  });

  const handleValueChange = React.useCallback(
    ({ value }: { value: string | null }) => {
      field.onChange(value);
      onValueChange?.({ value });
    },
    [ field, onValueChange ],
  );

  return (
    <RadioGroup
      ref={ field.ref }
      name={ field.name }
      value={ field.value }
      onValueChange={ handleValueChange }
      disabled={ formState.isSubmitting || disabled }
      { ...rest }
    >
      { options.map(({ value, label }) => (
        <Radio
          key={ value }
          value={ value }
          inputProps={{ onBlur: field.onBlur }}
          { ...itemProps }
        >
          { label }
        </Radio>
      )) }
    </RadioGroup>
  );
};

export const FormFieldRadio = React.memo(FormFieldRadioContent) as typeof FormFieldRadioContent;
