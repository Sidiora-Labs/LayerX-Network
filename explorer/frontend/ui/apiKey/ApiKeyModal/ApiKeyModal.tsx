import React, { useCallback, useState } from 'react';

import type { ApiKey } from 'types/api/account';

import FormModal from 'ui/shared/FormModal';

import ApiKeyForm from './ApiKeyForm';

type Props = {
  open: boolean;
  onOpenChange: ({ open }: { open: boolean }) => void;
  data?: ApiKey;
};

const ApiKeyModal: React.FC<Props> = ({ open, onOpenChange, data }) => {
  const title = data ? 'Edit API key' : 'New API key';
  const text = !data ? 'Add an application name to identify your API key. Click the button below to auto-generate the associated key.' : '';

  const [ isAlertVisible, setAlertVisible ] = useState(false);

  const renderForm = useCallback(() => {
    return <ApiKeyForm data={ data } onOpenChange={ onOpenChange } setAlertVisible={ setAlertVisible }/>;
  }, [ data, onOpenChange ]);
  return (
    <FormModal<ApiKey>
      open={ open }
      onOpenChange={ onOpenChange }
      title={ title }
      text={ text }
      renderForm={ renderForm }
      isAlertVisible={ isAlertVisible }
      setAlertVisible={ setAlertVisible }
    />
  );
};

export default ApiKeyModal;
