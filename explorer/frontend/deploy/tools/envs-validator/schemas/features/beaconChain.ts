import * as yup from 'yup';

export const beaconChainSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_HAS_BEACON_CHAIN: yup.boolean(),
    NEXT_PUBLIC_BEACON_CHAIN_WITHDRAWALS_ONLY: yup
      .boolean()
      .when('NEXT_PUBLIC_HAS_BEACON_CHAIN', {
        is: (value: boolean) => value,
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_BEACON_CHAIN_WITHDRAWALS_ONLY can only be used if NEXT_PUBLIC_HAS_BEACON_CHAIN is set to "true"',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_BEACON_CHAIN_CURRENCY_SYMBOL: yup
      .string()
      .when('NEXT_PUBLIC_HAS_BEACON_CHAIN', {
        is: (value: boolean) => value,
        then: (schema) => schema.min(1).optional(),
        otherwise: (schema) => schema.max(
          -1,
          'NEXT_PUBLIC_BEACON_CHAIN_CURRENCY_SYMBOL cannot not be used if NEXT_PUBLIC_HAS_BEACON_CHAIN is not set to "true"',
        ),
      }),
    NEXT_PUBLIC_BEACON_CHAIN_VALIDATOR_URL_TEMPLATE: yup
      .string()
      .when('NEXT_PUBLIC_HAS_BEACON_CHAIN', {
        is: (value: boolean) => value,
        then: (schema) => schema,
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_BEACON_CHAIN_VALIDATOR_URL_TEMPLATE cannot not be used if NEXT_PUBLIC_HAS_BEACON_CHAIN is not set to "true"'),
      }),
  });