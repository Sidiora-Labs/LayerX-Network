import React from 'react';

import type { Fields } from '../types';

import type { FieldProps } from 'toolkit/chakra/field';
import { FormFieldText } from 'toolkit/components/forms/fields/FormFieldText';
import { emailValidator } from 'toolkit/components/forms/validators/email';
import { urlValidator } from 'toolkit/components/forms/validators/url';

interface Props {
  readOnly?: boolean;
  size?: FieldProps['size'];
}

const TokenInfoFieldSupport = (props: Props) => {
  const validate = React.useCallback((newValue: string | undefined) => {
    if (typeof newValue !== 'string') {
      return true;
    }

    const urlValidationResult = urlValidator(newValue);
    const emailValidationResult = emailValidator(newValue);

    if (urlValidationResult === true || emailValidationResult === true) {
      return true;
    }

    return 'Invalid format';
  }, []);

  return (
    <FormFieldText<Fields, 'support'>
      name="support"
      placeholder="Support URL or email"
      rules={{ validate }}
      { ...props }
    />
  );
};

export default React.memo(TokenInfoFieldSupport);
