import { compile } from 'path-to-regexp';

import getResourceParams from 'lib/api/getResourceParams';
import type { ResourceName } from 'lib/api/resources';

export default function buildUrl(
  _resource: ResourceName,
  pathParams?: Record<string, string | undefined>,
  queryParams?: Record<string, string | number | undefined>,
) {
  const { resource, api } = getResourceParams(_resource);
  const baseUrl = api.endpoint;
  const basePath = api.basePath ?? '';
  const path = basePath + resource.path;
  const url = new URL(compile(path)(pathParams), baseUrl);

  queryParams && Object.entries(queryParams).forEach(([ key, value ]) => {
    value && url.searchParams.append(key, String(value));
  });

  return url.toString();
}
