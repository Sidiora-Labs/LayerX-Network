import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';

import { isValidVerificationMethod, sortVerificationMethods } from './utils';

export default function useFormConfigQuery(enabled: boolean) {
  return useApiQuery('general:contract_verification_config', {
    queryOptions: {
      select: (data) => {
        return {
          ...data,
          verification_options: [
            ...data.verification_options,
            ...config.UI.views.address.extraVerificationMethods,
          ].filter(isValidVerificationMethod).sort(sortVerificationMethods),
        };
      },
      enabled,
    },
  });
}
