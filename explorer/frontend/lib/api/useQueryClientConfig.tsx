import { QueryClient } from '@tanstack/react-query';
import React from 'react';

import getErrorObjPayload from 'lib/errors/getErrorObjPayload';
import getErrorObjStatusCode from 'lib/errors/getErrorObjStatusCode';

import type { ResourceName } from './resources';

export const retry = (failureCount: number, error: unknown) => {
  const errorPayload = getErrorObjPayload<{ status: number }>(error);
  const status = errorPayload?.status || getErrorObjStatusCode(error);
  if (status && status >= 400 && status < 500) {
    // don't do retry for client error responses
    return false;
  }
  return failureCount < 2;
};

export default function useQueryClientConfig() {
  const [ queryClient ] = React.useState(() => new QueryClient({
    defaultOptions: {
      queries: {
        refetchOnWindowFocus: false,
        retry,
        throwOnError: (error, query) => {
          const status = getErrorObjStatusCode(error);

          // we don't catch error only for "Too many requests" response
          if (status !== 429) {
            return false;
          }

          const EXTERNAL_API_RESOURCES: Array<ResourceName> = [
            'general:contract_solidity_scan_report',
            'general:address_xstar_score',
            'general:address_3rd_party_info',
            'general:noves_transaction',
            'general:noves_address_history',
            'general:noves_describe_txs',
            // these resources are not proxied by the backend
            'external:safe_transaction_api',
          ];
          const isExternalApiResource = EXTERNAL_API_RESOURCES.some((resource) => query.queryKey[0] === resource);

          return !isExternalApiResource;
        },
      },
    },
  }));

  return queryClient;
}
