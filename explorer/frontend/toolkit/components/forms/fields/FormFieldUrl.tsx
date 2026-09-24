import React from 'react';
import type { FieldValues } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';

import { urlValidator } from '../validators/url';
import { FormFieldText } from './FormFieldText';

const FormFieldUrlContent = <FormFields extends FieldValues>(
  props: FormFieldPropsBase<FormFields>,
) => {
  const rules = React.useMemo(
    () => ({
      ...props.rules,
      validate: {
        ...props.rules?.validate,
        url: urlValidator,
      },
    }),
    [ props.rules ],
  );

  return <FormFieldText { ...props } rules={ rules }/>;
};

export const FormFieldUrl = React.memo(FormFieldUrlContent) as typeof FormFieldUrlContent;
