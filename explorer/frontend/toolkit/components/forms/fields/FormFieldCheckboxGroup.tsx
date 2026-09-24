import React from 'react';
import { useController, useFormContext, type FieldValues, type Path } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';

import type { CheckboxGroupProps, CheckboxProps } from '../../../chakra/checkbox';
import { Checkbox, CheckboxGroup } from '../../../chakra/checkbox';

export interface FormFieldCheckboxGroupProps<
  FormFields extends FieldValues,
  Name extends Path<FormFields> = Path<FormFields>,
> extends Pick<FormFieldPropsBase<FormFields, Name>, 'rules' | 'name' | 'onChange' | 'readOnly' | 'controllerProps'>,
  Omit<CheckboxGroupProps, 'name' | 'onChange'> {
  options: Array<{ label: string; value: string }>;
  itemProps?: CheckboxProps;
}

const FormFieldCheckboxGroupContent = <
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
>(props: FormFieldCheckboxGroupProps<FormFields, Name>) => {
  const { name, options, disabled, controllerProps, itemProps, rules, onChange, ...rest } = props;

  const { control } = useFormContext<FormFields>();
  const { field, formState } = useController<FormFields, typeof name>({
    control,
    name,
    rules,
    ...controllerProps,
  });

  const handleChange = React.useCallback((value: Array<string>) => {
    field.onChange(value);
    onChange?.();
  }, [ field, onChange ]);

  return (
    <CheckboxGroup
      ref={ field.ref }
      name={ field.name }
      value={ field.value }
      onValueChange={ handleChange }
      disabled={ formState.isSubmitting || disabled }
      { ...rest }
    >
      { options.map(({ value, label }) => (
        <Checkbox key={ value } value={ value } { ...itemProps }>
          { label }
        </Checkbox>
      )) }
    </CheckboxGroup>
  );
};

export const FormFieldCheckboxGroup = React.memo(FormFieldCheckboxGroupContent) as typeof FormFieldCheckboxGroupContent;
