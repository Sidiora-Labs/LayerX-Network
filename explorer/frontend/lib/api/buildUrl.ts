import { compile } from 'path-to-regexp';

import type { ExternalChainExtended } from 'types/externalChains';

import config from 'configs/app';

import getResourceParams from './getResourceParams';
import isNeedProxy from './isNeedProxy';
import type { ResourceName, ResourcePathParams } from './resources';

export default function buildUrl<R extends ResourceName>(
  resourceFullName: R,
  pathParams?: ResourcePathParams<R>,
  queryParams?: Record<string, string | Array<string> | number | boolean | null | undefined>,
  noProxy?: boolean,
  chain?: ExternalChainExtended,
): string {
  const { api, resource } = getResourceParams(resourceFullName, chain);
  const baseUrl = !noProxy && isNeedProxy() ? config.app.baseUrl : api.endpoint;
  const basePath = api.basePath ?? '';
  const path = !noProxy && isNeedProxy() ? '/node-api/proxy' + basePath + resource.path : basePath + resource.path;
  const url = new URL(compile(path)(pathParams), baseUrl);

  queryParams && Object.entries(queryParams).forEach(([ key, value ]) => {
    // there are some pagination params that can be null or false for the next page
    value !== undefined && value !== '' && url.searchParams.append(key, String(value));
  });

  return url.toString();
}
