import React, { useCallback, useState } from 'react';

import type { WatchlistAddress } from 'types/api/account';

import FormModal from 'ui/shared/FormModal';

import AddressForm from './AddressForm';

type Props = {
  isAdd: boolean;
  open: boolean;
  onOpenChange: ({ open }: { open: boolean }) => void;
  onSuccess: () => Promise<void>;
  data?: Partial<WatchlistAddress>;
  hasEmail: boolean;
  showEmailAlert?: boolean;
};

const AddressModal: React.FC<Props> = ({ open, onOpenChange, onSuccess, data, isAdd, hasEmail, showEmailAlert }) => {
  const title = !isAdd ? 'Edit watch list address' : 'New address to watch list';
  const text = isAdd ? 'An email notification can be sent to you when an address on your watch list sends or receives any transactions.' : '';

  const [ isAlertVisible, setAlertVisible ] = useState(false);

  const renderForm = useCallback(() => {
    return (
      <AddressForm
        data={ data }
        onSuccess={ onSuccess }
        setAlertVisible={ setAlertVisible }
        isAdd={ isAdd }
        hasEmail={ hasEmail }
        showEmailAlert={ showEmailAlert }
      />
    );
  }, [ data, isAdd, onSuccess, hasEmail, showEmailAlert ]);

  return (
    <FormModal<WatchlistAddress>
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

export default AddressModal;
