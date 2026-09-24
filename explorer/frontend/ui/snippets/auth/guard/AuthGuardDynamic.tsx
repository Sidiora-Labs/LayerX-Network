import { useDynamicContext, useDynamicEvents } from '@dynamic-labs/sdk-react-core';
import React from 'react';

import useLinkEmailDynamic from '../linkEmail/useLinkEmailDynamic';
import useProfileQuery from '../useProfileQuery';

interface InjectedProps {
  onClick: () => void;
}

interface Props {
  children: (props: InjectedProps) => React.ReactNode;
  onAuthSuccess: () => void;
  ensureEmail?: boolean;
}

const AuthGuardDynamic = ({ children, onAuthSuccess, ensureEmail }: Props) => {
  const isAuthStartedRef = React.useRef(false);
  const profileQuery = useProfileQuery();
  const { setShowAuthFlow } = useDynamicContext();
  const { user } = useDynamicContext();

  const linkEmailToProfile = useLinkEmailDynamic();

  const handleClick = React.useCallback(() => {
    if (profileQuery.data) {
      if (ensureEmail && !profileQuery.data?.email) {
        linkEmailToProfile();
      } else {
        onAuthSuccess();
      }
    } else {
      setShowAuthFlow(true);
      isAuthStartedRef.current = true;
    }
  }, [ profileQuery.data, ensureEmail, linkEmailToProfile, onAuthSuccess, setShowAuthFlow ]);

  const onAuthFlowEnd = React.useCallback(() => {
    if (!user || !isAuthStartedRef.current) {
      return;
    }

    isAuthStartedRef.current = false;
    if (ensureEmail && !user?.email) {
      linkEmailToProfile();
    } else {
      onAuthSuccess();
    }
  }, [ ensureEmail, linkEmailToProfile, onAuthSuccess, user ]);

  // This event is triggered when user is logged in with wallet
  useDynamicEvents('authFlowClose', async() => {
    onAuthFlowEnd();
  });

  // This event is triggered when user is logged in with email
  useDynamicEvents('emailVerificationResult', () => {
    onAuthFlowEnd();
  });

  return children({ onClick: handleClick });
};

export default React.memo(AuthGuardDynamic);
