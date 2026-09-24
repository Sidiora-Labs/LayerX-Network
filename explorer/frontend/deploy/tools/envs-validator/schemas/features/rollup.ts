import * as yup from 'yup';
import { urlTest, getYupValidationErrorMessage } from '../../utils';
import { replaceQuotes } from 'configs/app/utils';
import * as regexp from 'toolkit/utils/regexp';
import { ROLLUP_TYPES } from 'types/client/rollup';

const parentChainCurrencySchema = yup
  .object()
  .shape({
    name: yup.string().required(),
    symbol: yup.string().required(),
    decimals: yup.number().required(),
  });

export const rollupSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_ROLLUP_TYPE: yup.string().oneOf(ROLLUP_TYPES),
    NEXT_PUBLIC_ROLLUP_PARENT_CHAIN: yup
      .object()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => value,
        then: (schema) => {
          return schema.transform(replaceQuotes).json().shape({
            id: yup.number(),
            name: yup.string(),
            baseUrl: yup.string().test(urlTest).required(),
            rpcUrls: yup.array().of(yup.string().test(urlTest)),
            currency: yup
              .mixed()
              .test(
                'shape',
                (ctx) => {
                  try {
                    parentChainCurrencySchema.validateSync(ctx.originalValue);
                    throw new Error('Unknown validation error');
                  } catch (error: unknown) {
                    const message = getYupValidationErrorMessage(error);
                    return 'in \"currency\" property ' + (message ? `${ message }` : '');
                  }
                },
                (data) => {
                  const isUndefined = data === undefined;
                  return isUndefined || parentChainCurrencySchema.isValidSync(data);
                },
              ),
            isTestnet: yup.boolean(),
          })
        },
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN cannot not be used if NEXT_PUBLIC_ROLLUP_TYPE is not defined',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_ROLLUP_L2_WITHDRAWAL_URL: yup
      .string()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => value === 'optimistic',
        then: (schema) => schema.test(urlTest),
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_ROLLUP_L2_WITHDRAWAL_URL can be used only if NEXT_PUBLIC_ROLLUP_TYPE is set to \'optimistic\' '),
      }),
    NEXT_PUBLIC_ROLLUP_OUTPUT_ROOTS_ENABLED: yup
      .boolean()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: 'optimistic',
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_ROLLUP_OUTPUT_ROOTS_ENABLED can only be used if NEXT_PUBLIC_ROLLUP_TYPE is set to \'optimistic\' ',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_INTEROP_ENABLED: yup
      .boolean()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: 'optimistic',
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_INTEROP_ENABLED can only be used if NEXT_PUBLIC_ROLLUP_TYPE is set to \'optimistic\' ',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_FAULT_PROOF_ENABLED: yup.boolean()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: 'optimistic',
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_FAULT_PROOF_ENABLED can only be used with NEXT_PUBLIC_ROLLUP_TYPE=optimistic',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_HAS_MUD_FRAMEWORK: yup.boolean()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: 'optimistic',
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_HAS_MUD_FRAMEWORK can only be used with NEXT_PUBLIC_ROLLUP_TYPE=optimistic',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_ROLLUP_HOMEPAGE_SHOW_LATEST_BLOCKS: yup
      .boolean()
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => value,
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_ROLLUP_HOMEPAGE_SHOW_LATEST_BLOCKS cannot not be used if NEXT_PUBLIC_ROLLUP_TYPE is not defined',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_ROLLUP_DA_CELESTIA_NAMESPACE: yup
      .string()
      .min(60)
      .max(60)
      .matches(regexp.HEX_REGEXP_WITH_0X)
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => value === 'arbitrum',
        then: (schema) => schema,
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_ROLLUP_DA_CELESTIA_NAMESPACE can only be used if NEXT_PUBLIC_ROLLUP_TYPE is set to \'arbitrum\' '),
      }),
    NEXT_PUBLIC_ROLLUP_DA_CELESTIA_CELENIUM_URL: yup
      .string()
      .test(urlTest)
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => value === 'arbitrum' || value === 'optimistic',
        then: (schema) => schema,
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_ROLLUP_DA_CELESTIA_CELENIUM_URL can only be used if NEXT_PUBLIC_ROLLUP_TYPE is set to \'arbitrum\' or \'optimistic\''),
      }),
    NEXT_PUBLIC_ROLLUP_STAGE_INDEX: yup.number().oneOf([ 1, 2 ])
      .when('NEXT_PUBLIC_ROLLUP_TYPE', {
        is: (value: string) => Boolean(value),
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_ROLLUP_STAGE_INDEX can only be used with NEXT_PUBLIC_ROLLUP_TYPE',
          value => value === undefined,
        ),
      }),
      NEXT_PUBLIC_ROLLUP_LAYER_NUMBER: yup.number()
        .positive()
        .integer()
        .min(2)
        .when('NEXT_PUBLIC_ROLLUP_TYPE', {
          is: (value: string) => Boolean(value),
          then: (schema) => schema,
          otherwise: (schema) => schema.test(
            'not-exist',
            'NEXT_PUBLIC_ROLLUP_LAYER_NUMBER can only be used with NEXT_PUBLIC_ROLLUP_TYPE',
            value => value === undefined,
          ),
        }),
  });