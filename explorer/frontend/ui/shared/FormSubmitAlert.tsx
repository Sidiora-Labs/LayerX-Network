import React from 'react';

import { Alert } from 'toolkit/chakra/alert';

const FormSubmitAlert = () => {
  return (
    <Alert status="error">
      There has been an error processing your request
    </Alert>
  );
};

export default FormSubmitAlert;
