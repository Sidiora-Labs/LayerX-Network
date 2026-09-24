import fetch, { AbortError } from 'node-fetch';

import buildUrl from 'nextjs/utils/buildUrl';
import { httpLogger } from 'nextjs/utils/logger';

import type { ResourceName, ResourcePathParams, ResourcePayload } from 'lib/api/resources';
import metrics from 'lib/monitoring/metrics';
import { SECOND } from 'toolkit/utils/consts';

type Params<R extends ResourceName> = {
  resource: R;
  pathParams?: ResourcePathParams<R>;
  queryParams?: Record<string, string | number | undefined>;
  timeout?: number;
};

export default async function fetchApi<R extends ResourceName = never, S = ResourcePayload<R>>(params: Params<R>): Promise<S | undefined> {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort();
  }, params.timeout || SECOND);

  const url = buildUrl(params.resource, params.pathParams, params.queryParams);

  const end = metrics?.apiRequestDuration.startTimer();

  try {
    const response = await fetch(url, { signal: controller.signal });

    const duration = end?.({ route: params.resource, code: response.status });
    if (response.status === 200) {
      httpLogger.logger.info({ message: 'API fetch', url, code: response.status, duration });
    } else {
      httpLogger.logger.error({ message: 'API fetch', url, code: response.status, duration });
    }

    return await response.json() as Promise<S>;
  } catch (error) {
    const code = error instanceof AbortError ? 504 : 500;
    const duration = end?.({ route: params.resource, code });
    httpLogger.logger.error({ message: 'API fetch', url, code, duration });
  } finally {
    clearTimeout(timeout);
  }
}
