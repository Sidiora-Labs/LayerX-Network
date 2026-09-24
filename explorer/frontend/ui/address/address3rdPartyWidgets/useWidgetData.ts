import { get } from 'es-toolkit/compat';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';

const RESOURCE_NAME = 'general:address_3rd_party_info';

const formatValue = (value: unknown): string | undefined => {
  if (typeof value !== 'number' && typeof value !== 'string') {
    return undefined;
  }

  const num = Number(value);
  if (!isNaN(num)) {
    if (num === -1) {
      return '0';
    }
    return num.toLocaleString();
  }

  return String(value);
};

export default function useWidgetData(name: string, valuePath: string | undefined, address: string, isLoading: boolean) {
  const query = useApiQuery<typeof RESOURCE_NAME, unknown, string | undefined>(RESOURCE_NAME, {
    pathParams: { name },
    queryParams: { address, chain_id: config.chain.id },
    queryOptions: {
      select: (response) => {
        try {
          const value = get(response, valuePath || '');
          return formatValue(value);
        } catch {
          return undefined;
        }
      },
      enabled: !isLoading && Boolean(valuePath),
      refetchOnMount: false,
    },
  });

  return query;
}
