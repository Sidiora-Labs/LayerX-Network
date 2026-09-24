import * as yup from 'yup';
import { urlTest } from '../../utils';

export const tacSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_TAC_OPERATION_LIFECYCLE_API_HOST: yup.string().test(urlTest),
    NEXT_PUBLIC_TAC_TON_EXPLORER_URL: yup
      .string()
      .when('NEXT_PUBLIC_TAC_OPERATION_LIFECYCLE_API_HOST', {
        is: (value: string) => Boolean(value),
        then: (schema) => schema.test(urlTest),
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_TAC_TON_EXPLORER_URL can only be used with NEXT_PUBLIC_TAC_OPERATION_LIFECYCLE_API_HOST',
          value => value === undefined,
        ),
      }),
  });