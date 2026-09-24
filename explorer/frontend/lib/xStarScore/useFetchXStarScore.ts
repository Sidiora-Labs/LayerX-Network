import React from 'react';
import * as v from 'valibot';

import config from 'configs/app';
import buildUrl from 'lib/api/buildUrl';
import useApiQuery from 'lib/api/useApiQuery';

interface Params {
  hash: string;
}

const RESOURCE_NAME = 'general:address_xstar_score';
const ERROR_NAME = 'Invalid response schema';

export default function useFetchXStarScore({ hash }: Params) {
  const query = useApiQuery(RESOURCE_NAME, {
    pathParams: { hash },
    queryOptions: {
      select: (response) => {
        const parsedResponse = v.safeParse(v.object({ data: v.object({ level: v.nullable(v.string()) }) }), response);

        if (!parsedResponse.success) {
          throw Error(ERROR_NAME);
        }

        return parsedResponse.output;
      },
      enabled: Boolean(hash) && config.features.xStarScore.isEnabled,
      placeholderData: {
        data: { level: 'Base' },
      },
      retry: 0,
    },
  });

  const errorMessage = query.error && 'message' in query.error ? query.error.message : undefined;

  React.useEffect(() => {
    if (errorMessage === ERROR_NAME) {
      fetch('/node-api/monitoring/invalid-api-schema', {
        method: 'POST',
        body: JSON.stringify({
          resource: RESOURCE_NAME,
          url: buildUrl(RESOURCE_NAME, { hash }, undefined, true),
        }),
      });
    }
  }, [ errorMessage, hash ]);

  return query;
}
