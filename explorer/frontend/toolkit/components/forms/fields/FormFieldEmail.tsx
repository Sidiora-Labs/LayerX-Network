import React from 'react';
import type { FieldValues } from 'react-hook-form';

import type { FormFieldPropsBase } from './types';
import type { PartialBy } from 'types/utils';

import { EMAIL_REGEXP } from '../validators/email';
import { FormFieldText } from './FormFieldText';

const FormFieldEmailContent = <FormFields extends FieldValues>(
  props: PartialBy<FormFieldPropsBase<FormFields>, 'placeholder'>,
) => {
  const rules = React.useMemo(
    () => ({
      ...props.rules,
      pattern: EMAIL_REGEXP,
    }),
    [ props.rules ],
  );

  return (
    <FormFieldText
      { ...props }
      placeholder={ props.placeholder || 'Email' }
      rules={ rules }
    />
  );
};

export const FormFieldEmail = React.memo(FormFieldEmailContent) as typeof FormFieldEmailContent;
