import React from 'react';
import type { FieldValues, Path } from 'react-hook-form';
import { useController, useFormContext } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';

import { Field } from '../../../chakra/field';
import type { InputProps } from '../../../chakra/input';
import { Input } from '../../../chakra/input';
import { InputGroup } from '../../../chakra/input-group';
import type { TextareaProps } from '../../../chakra/textarea';
import { Textarea } from '../../../chakra/textarea';
import { getFormFieldErrorText } from '../utils/getFormFieldErrorText';

export interface FormFieldTextProps<
  FormFields extends FieldValues,
  Name extends Path<FormFields> = Path<FormFields>,
> extends FormFieldPropsBase<FormFields, Name> {
  asComponent?: 'Input' | 'Textarea';
}

const FormFieldTextContent = <
  FormFields extends FieldValues,
  Name extends Path<FormFields> = Path<FormFields>,
>({
  name,
  placeholder,
  rules,
  onBlur,
  group,
  inputProps,
  asComponent,
  size: sizeProp,
  disabled,
  floating: floatingProp,
  controllerProps,
  ...restProps
}: FormFieldTextProps<FormFields, Name>) => {
  const defaultSize = asComponent === 'Textarea' ? '2xl' : 'lg';
  const size = sizeProp || defaultSize;
  const floating = floatingProp !== undefined ? floatingProp : size === defaultSize;

  const { control } = useFormContext<FormFields>();
  const { field, fieldState, formState } = useController<FormFields, typeof name>({
    control,
    name,
    rules: { ...rules, required: restProps.required },
    ...controllerProps,
  });

  const handleBlur = React.useCallback(() => {
    field.onBlur();
    onBlur?.();
  }, [ field, onBlur ]);

  const input = asComponent === 'Textarea' ? (
    <Textarea
      { ...field }
      autoComplete="off"
      flexGrow={ 1 }
      { ...inputProps as TextareaProps }
      onBlur={ handleBlur }
    />
  ) : (
    <Input
      { ...field }
      autoComplete="off"
      // for non-floating field label, we pass placeholder to the input component
      placeholder={ !floating ? placeholder : undefined }
      { ...inputProps as InputProps }
      onBlur={ handleBlur }
    />
  );

  const content = group ? (
    <InputGroup
      { ...group }
      endElement={ typeof group.endElement === 'function' ? group.endElement({ field }) : group.endElement }
    >
      { input }
    </InputGroup>
  ) : input;

  return (
    <Field
      // for floating field label, we pass placeholder value to the label
      label={ floating ? placeholder : undefined }
      errorText={ getFormFieldErrorText(fieldState.error) }
      invalid={ Boolean(fieldState.error) }
      disabled={ formState.isSubmitting || disabled }
      size={ size }
      floating={ floating }
      { ...restProps }
    >
      { content }
    </Field>
  );
};

export const FormFieldText = React.memo(FormFieldTextContent) as typeof FormFieldTextContent;
