import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import * as cookies from 'lib/cookies';

export default function useProfileQuery() {
  return useApiQuery('general:user_info', {
    queryOptions: {
      refetchOnMount: false,
      enabled: config.features.account.isEnabled && Boolean(cookies.get(cookies.NAMES.API_TOKEN)),
    },
  });
}
