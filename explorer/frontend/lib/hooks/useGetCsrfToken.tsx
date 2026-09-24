import { useQuery } from '@tanstack/react-query';

import buildUrl from 'lib/api/buildUrl';
import isNeedProxy from 'lib/api/isNeedProxy';
import { getResourceKey } from 'lib/api/useApiQuery';
import * as cookies from 'lib/cookies';
import useFetch from 'lib/hooks/useFetch';

export default function useGetCsrfToken() {
  const nodeApiFetch = useFetch();

  return useQuery({
    queryKey: getResourceKey('general:csrf'),
    queryFn: async() => {
      if (!isNeedProxy()) {
        const url = buildUrl('general:csrf');
        const apiResponse = await fetch(url, { credentials: 'include' });
        const csrfFromHeader = apiResponse.headers.get('x-bs-account-csrf');

        if (!csrfFromHeader) {
          // I am not sure should we log this error or not
          // so I commented it out for now
          // rollbar?.warn('Client fetch failed', {
          //   resource: 'csrf',
          //   status_code: 500,
          //   status_text: 'Unable to obtain csrf token from header',
          // });
          return;
        }

        return { token: csrfFromHeader };
      }

      return nodeApiFetch('/node-api/csrf');
    },
    enabled: Boolean(cookies.get(cookies.NAMES.API_TOKEN)),
  });
}
