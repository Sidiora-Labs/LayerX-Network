import type { ArrayElement } from 'types/utils';

import type { PaginatedResourceName, PaginatedResourceResponse, PaginatedResourceResponseItems } from 'lib/api/resources';

export function generateListStub<Resource extends PaginatedResourceName>(
  stub: ArrayElement<PaginatedResourceResponseItems<Resource>>,
  num = 50,
  rest: Omit<PaginatedResourceResponse<Resource>, 'items'>,
) {
  return {
    items: Array(num).fill(stub),
    ...rest,
  };
}
