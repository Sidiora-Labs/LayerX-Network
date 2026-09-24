import * as yup from 'yup';
import { urlTest } from '../../utils';

export const userOpsSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_HAS_USER_OPS: yup.boolean(),
    NEXT_PUBLIC_USER_OPS_INDEXER_API_HOST: yup
      .string()
      .test(urlTest)
      .when('NEXT_PUBLIC_HAS_USER_OPS', {
        is: (value: boolean) => value,
        then: (schema) => schema,
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_USER_OPS_INDEXER_API_HOST can only be used if NEXT_PUBLIC_HAS_USER_OPS is set to \'true\''),
      }),
  });