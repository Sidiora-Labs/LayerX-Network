import React from 'react';
import type { FieldValues } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';
import type { PartialBy } from 'types/utils';

import { addressValidator } from '../validators/address';
import { FormFieldText } from './FormFieldText';

const FormFieldAddressContent = <FormFields extends FieldValues>(
  props: PartialBy<FormFieldPropsBase<FormFields>, 'placeholder'>,
) => {
  const rules = React.useMemo(
    () => ({
      ...props.rules,
      validate: {
        ...props.rules?.validate,
        address: addressValidator,
      },
    }),
    [ props.rules ],
  );

  return (
    <FormFieldText
      { ...props }
      placeholder={ props.placeholder || 'Address (0x...)' }
      rules={ rules }
    />
  );
};

export const FormFieldAddress = React.memo(FormFieldAddressContent) as typeof FormFieldAddressContent;
