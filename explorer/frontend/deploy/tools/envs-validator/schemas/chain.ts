import * as yup from 'yup';
import { urlTest } from '../utils';
import { replaceQuotes } from 'configs/app/utils';
import type { NetworkVerificationTypeEnvs } from 'types/networks';

// Blockchain parameters schema
export default yup.object({
    NEXT_PUBLIC_NETWORK_NAME: yup.string().required(),
    NEXT_PUBLIC_NETWORK_SHORT_NAME: yup.string(),
    NEXT_PUBLIC_NETWORK_ID: yup.number().positive().integer().required(),
    NEXT_PUBLIC_NETWORK_RPC_URL: yup
    .mixed()
    .test(
      'shape',
      'Invalid schema were provided for NEXT_PUBLIC_NETWORK_RPC_URL, it should be either array of URLs or URL string',
      (data) => {
        const isUrlSchema = yup.string().test(urlTest);
        const isArrayOfUrlsSchema = yup
          .array()
          .transform(replaceQuotes)
          .json()
          .of(yup.string().test(urlTest));

        return isUrlSchema.isValidSync(data) || isArrayOfUrlsSchema.isValidSync(data);
      }),
    NEXT_PUBLIC_NETWORK_CURRENCY_NAME: yup.string(),
    NEXT_PUBLIC_NETWORK_CURRENCY_WEI_NAME: yup.string(),
    NEXT_PUBLIC_NETWORK_CURRENCY_GWEI_NAME: yup.string(),
    NEXT_PUBLIC_NETWORK_CURRENCY_SYMBOL: yup.string(),
    NEXT_PUBLIC_NETWORK_CURRENCY_DECIMALS: yup.number().integer().positive(),
    NEXT_PUBLIC_NETWORK_SECONDARY_COIN_SYMBOL: yup.string(),
    NEXT_PUBLIC_NETWORK_MULTIPLE_GAS_CURRENCIES: yup.boolean(),
    NEXT_PUBLIC_NETWORK_VERIFICATION_TYPE: yup
      .string<NetworkVerificationTypeEnvs>().oneOf([ 'validation', 'mining', 'fee reception' ])
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => value === 'arbitrum' || value === 'zkEvm',
        then: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_NETWORK_VERIFICATION_TYPE can not be set for Arbitrum and ZkEVM rollups',
          value => value === undefined,
        ),
        otherwise: (schema) => schema,
      }),
    NEXT_PUBLIC_NETWORK_TOKEN_STANDARD_NAME: yup.string(),
    NEXT_PUBLIC_NETWORK_ADDITIONAL_TOKEN_TYPES: yup
      .array()
      .transform(replaceQuotes)
      .json()
      .of(yup.object({
        id: yup.string().required(),
        name: yup.string().required(),
      }).noUnknown(true)),
    NEXT_PUBLIC_IS_TESTNET: yup.boolean(),
});