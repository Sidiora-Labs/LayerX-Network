import { AuthProvider } from 'types/client/account';
import * as yup from 'yup';
import { urlTest } from '../../utils';

export const accountSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED: yup.boolean(),
    NEXT_PUBLIC_ACCOUNT_API_KEYS_BUTTON: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_ACCOUNT_API_KEYS_BUTTON, it should be either boolean or string', (data) => {
        return yup.boolean().isValidSync(data) || yup.string().test(urlTest).isValidSync(data);
      })
      .when('NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED', {    
        is: (value: boolean) => value === true,
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_ACCOUNT_API_KEYS_BUTTON can only be used if NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED is set to \'true\'',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_ACCOUNT_AUTH_PROVIDER: yup
      .string<AuthProvider>()
      .when('NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED', {
        is: (value: boolean) => value === true,
        then: (schema) => schema.oneOf([ 'auth0', 'dynamic' ]),
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_ACCOUNT_AUTH_PROVIDER can only be used if NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED is set to \'true\''),
      }),
    NEXT_PUBLIC_ACCOUNT_DYNAMIC_ENVIRONMENT_ID: yup
      .string()
      .when('NEXT_PUBLIC_ACCOUNT_AUTH_PROVIDER', {
        is: (value: AuthProvider) => value === 'dynamic',
        then: (schema) => schema.required(),
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_ACCOUNT_DYNAMIC_ENVIRONMENT_ID can only be used if NEXT_PUBLIC_ACCOUNT_AUTH_PROVIDER is set to \'dynamic\' '),
      }),
  });