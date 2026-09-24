import * as yup from 'yup';
import { urlTest } from '../../utils';

export const crossChainTxsSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_CROSS_CHAIN_TXS_ENABLED: yup.boolean(),
    NEXT_PUBLIC_INTERCHAIN_INDEXER_API_HOST: yup
      .string()
      .when('NEXT_PUBLIC_CROSS_CHAIN_TXS_ENABLED', {
        is: (value: boolean) => value,
        then: (schema) => schema.test(urlTest),
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_INTERCHAIN_INDEXER_API_HOST can only be used with NEXT_PUBLIC_CROSS_CHAIN_TXS_ENABLED',
          value => value === undefined,
        ),
      }),
  });