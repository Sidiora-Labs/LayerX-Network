import React from 'react';

import config from 'configs/app';
import { Alert } from 'toolkit/chakra/alert';
import { Button } from 'toolkit/chakra/button';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AuthModal from 'ui/snippets/auth/AuthModal';
import useLinkEmail from 'ui/snippets/auth/linkEmail/useLinkEmail';

const feature = config.features.account;

const WatchlistEmailAlert = () => {
  const authModal = useDisclosure();
  const linkEmail = useLinkEmail();

  const handleButtonClick = React.useCallback(() => {
    if (feature.isEnabled && feature.authProvider === 'dynamic') {
      linkEmail();
    } else {
      authModal.onOpen();
    }
  }, [ authModal, linkEmail ]);

  return (
    <>
      <Alert
        status="info"
        descriptionProps={{ alignItems: 'center', gap: 2 }}
        w="fit-content"
        mb={ 6 }
      >
        To receive notifications you need to add an email to your profile.
        <Button variant="outline" size="sm" onClick={ handleButtonClick }>Add email</Button>
      </Alert>
      { authModal.open && <AuthModal initialScreen={{ type: 'email', isAuth: true }} onClose={ authModal.onClose }/> }
    </>
  );
};

export default React.memo(WatchlistEmailAlert);
