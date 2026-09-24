import React from 'react';

import type { FormFields } from '../types';

import { FormFieldEmail } from 'toolkit/components/forms/fields/FormFieldEmail';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  isReadOnly?: boolean;
  defaultValue: string | undefined;
}

const MyProfileFieldsEmail = ({ isReadOnly, defaultValue }: Props) => {

  return (
    <FormFieldEmail<FormFields>
      name="email"
      placeholder="Email"
      required
      readOnly={ isReadOnly }
      helperText="Email for watch list notifications and private tags"
      group={{
        endElement: ({ field }) => {
          const isVerified = defaultValue && field.value === defaultValue;
          return isVerified ? <IconSvg name="certified" boxSize={ 5 } color="green.500" mx={ 5 }/> : null;
        },
      }}
    />
  );
};

export default React.memo(MyProfileFieldsEmail);
