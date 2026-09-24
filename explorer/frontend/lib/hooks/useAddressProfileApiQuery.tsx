import { useQuery } from '@tanstack/react-query';
import * as v from 'valibot';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import useFetch from 'lib/hooks/useFetch';

const feature = config.features.addressProfileAPI;

type AddressInfoApiQueryResponse = v.InferOutput<typeof AddressInfoSchema>;

const AddressInfoSchema = v.object({
  user_profile: v.object({
    username: v.union([ v.string(), v.null() ]),
  }),
});

const ERROR_NAME = 'Invalid response schema';

export default function useAddressProfileApiQuery(hash: string | undefined, isEnabled = true) {
  const fetch = useFetch();

  return useQuery<unknown, ResourceError<unknown>, AddressInfoApiQueryResponse>({
    queryKey: [ 'username_api', hash ],
    queryFn: async() => {
      if (!feature.isEnabled || !hash) {
        return Promise.reject();
      }

      return fetch(feature.apiUrlTemplate.replace('{address}', hash));
    },
    enabled: isEnabled && Boolean(hash),
    refetchOnMount: false,
    select: (response) => {
      const parsedResponse = v.safeParse(AddressInfoSchema, response);

      if (!parsedResponse.success) {
        throw Error(ERROR_NAME);
      }

      return parsedResponse.output;
    },
  });
}
