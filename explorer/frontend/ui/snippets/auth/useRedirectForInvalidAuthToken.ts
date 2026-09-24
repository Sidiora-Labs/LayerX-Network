import React from 'react';

import * as cookies from 'lib/cookies';
import { useRollbar } from 'lib/rollbar';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';

export default function useRedirectForInvalidAuthToken() {
  const rollbar = useRollbar();
  const profileQuery = useProfileQuery();
  const errorStatus = profileQuery.error?.status;

  React.useEffect(() => {
    if (errorStatus === 401) {
      const apiToken = cookies.get(cookies.NAMES.API_TOKEN);

      if (apiToken) {
        cookies.remove(cookies.NAMES.API_TOKEN);
        window.location.assign('/');
      }
    }
  }, [ errorStatus, rollbar ]);
}
