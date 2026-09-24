import React from 'react';
import type { FieldValues } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';

import type { InputProps } from '../../../chakra/input';
import { FormFieldText } from './FormFieldText';

const FormFieldNumberContent = <FormFields extends FieldValues>(
  { inputProps, ...rest }: FormFieldPropsBase<FormFields>,
) => {
  return <FormFieldText { ...rest } inputProps={{ type: 'number', ...inputProps } as InputProps}/>;
};

export const FormFieldNumber = React.memo(FormFieldNumberContent) as typeof FormFieldNumberContent;
