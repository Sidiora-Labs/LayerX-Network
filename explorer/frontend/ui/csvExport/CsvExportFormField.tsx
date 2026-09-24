import { capitalize } from 'es-toolkit';
import React from 'react';
import type { UseFormReturn } from 'react-hook-form';

import type { FormFields } from './types';

import dayjs from 'lib/date/dayjs';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';

interface Props {
  formApi: UseFormReturn<FormFields>;
  name: 'from' | 'to';
}

const CsvExportFormField = ({ formApi, name }: Props) => {
  const { formState, getValues, trigger } = formApi;

  const validate = React.useCallback((newValue: string) => {
    if (name === 'from') {
      const toValue = getValues('to');
      if (toValue && dayjs(newValue) > dayjs(toValue)) {
        return 'Incorrect date';
      }
      if (formState.errors.to) {
        trigger('to');
      }
    } else {
      const fromValue = getValues('from');
      if (fromValue && dayjs(fromValue) > dayjs(newValue)) {
        return 'Incorrect date';
      }
      if (formState.errors.from) {
        trigger('from');
      }
    }
  }, [ formState.errors.from, formState.errors.to, getValues, name, trigger ]);

  return (
    <FormFieldText<FormFields, typeof name>
      name={ name }
      inputProps={{ type: 'datetime-local', max: dayjs().format('YYYY-MM-DDTHH:mm') }}
      placeholder={ capitalize(name) }
      required
      rules={{ validate }}
      maxW={{ base: 'auto', lg: '220px' }}
    />
  );
};

export default React.memo(CsvExportFormField);
