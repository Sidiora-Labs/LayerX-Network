/* eslint-disable max-len */
declare module 'yup' {
  interface StringSchema {
    // Yup's URL validator is not perfect so we made our own
    // https://github.com/jquense/yup/pull/1859
    url(): never;
  }
}

import * as yup from 'yup';

import type { AddressProfileAPIConfig } from 'types/client/addressProfileAPIConfig';
import type { GasRefuelProviderConfig } from 'types/client/gasRefuelProviderConfig';
import { GAS_UNITS } from 'types/client/gasTracker';
import type { GasUnit } from 'types/client/gasTracker';
import { PROVIDERS as TX_INTERPRETATION_PROVIDERS } from 'types/client/txInterpretation';
import { VALIDATORS_CHAIN_TYPE } from 'types/client/validators';
import type { ValidatorsChainType } from 'types/client/validators';
import type { WalletType } from 'types/client/wallets';
import { SUPPORTED_WALLETS } from 'types/client/wallets';
import type { TxExternalTxsConfig } from 'types/client/externalTxsConfig';

import { replaceQuotes } from 'configs/app/utils';
import { urlTest, protocols } from './utils';
import apisSchema from './schemas/apis';
import chainSchema from './schemas/chain';
import metaSchema from './schemas/meta';
import * as uiSchemas from './schemas/ui';
import * as featuresSchemas from './schemas/features';
import servicesSchema from './schemas/services';

const schema = yup
  .object()
  .noUnknown(true, (params) => {
    return `Unknown ENV variables were provided: ${ params.unknown }`;
  })
  .shape({
    // I. Build-time ENVs
    // -----------------
    NEXT_PUBLIC_GIT_TAG: yup.string(),
    NEXT_PUBLIC_GIT_COMMIT_SHA: yup.string(),

    // II. Run-time ENVs
    // -----------------
    // App configuration
    NEXT_PUBLIC_APP_HOST: yup.string().required(),
    NEXT_PUBLIC_APP_PROTOCOL: yup.string().oneOf(protocols),
    NEXT_PUBLIC_APP_PORT: yup.number().positive().integer(),
    NEXT_PUBLIC_APP_ENV: yup.string(),
    NEXT_PUBLIC_APP_INSTANCE: yup.string(),


    // Features configuration
    // NOTE: As a rule of thumb, only include features that require a single ENV variable here.
    // Otherwise, consider placing them in the corresponding schema file in the "./schemas/features" directory.
    NEXT_PUBLIC_WEB3_WALLETS: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_WEB3_WALLETS, it should be either array or "none" string literal', (data) => {
        const isNoneSchema = yup.string().equals([ 'none' ]);
        const isArrayOfWalletsSchema = yup
          .array()
          .transform(replaceQuotes)
          .json()
          .of(yup.string<WalletType>().oneOf(SUPPORTED_WALLETS));

        return isNoneSchema.isValidSync(data) || isArrayOfWalletsSchema.isValidSync(data);
      }),
    NEXT_PUBLIC_WEB3_DISABLE_ADD_TOKEN_TO_WALLET: yup.boolean(),
    NEXT_PUBLIC_TRANSACTION_INTERPRETATION_PROVIDER: yup.string().oneOf(TX_INTERPRETATION_PROVIDERS),
    NEXT_PUBLIC_SAFE_TX_SERVICE_URL: yup.string().test(urlTest),
    NEXT_PUBLIC_IS_SUAVE_CHAIN: yup.boolean(),
    NEXT_PUBLIC_METASUITES_ENABLED: yup.boolean(),
    NEXT_PUBLIC_GAS_REFUEL_PROVIDER_CONFIG: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_GAS_REFUEL_PROVIDER_CONFIG, it should have name and url template', (data) => {
        const isUndefined = data === undefined;
        const valueSchema = yup.object<GasRefuelProviderConfig>().transform(replaceQuotes).json().shape({
          name: yup.string().required(),
          url_template: yup.string().required(),
          logo: yup.string(),
          dapp_id: yup.string(),
        });

        return isUndefined || valueSchema.isValidSync(data);
      }),
    NEXT_PUBLIC_VALIDATORS_CHAIN_TYPE: yup.string<ValidatorsChainType>().oneOf(VALIDATORS_CHAIN_TYPE),
    NEXT_PUBLIC_GAS_TRACKER_ENABLED: yup.boolean(),
    NEXT_PUBLIC_GAS_TRACKER_UNITS: yup.array().transform(replaceQuotes).json().of(yup.string<GasUnit>().oneOf(GAS_UNITS)),
    NEXT_PUBLIC_DATA_AVAILABILITY_ENABLED: yup.boolean(),
    NEXT_PUBLIC_ADVANCED_FILTER_ENABLED: yup.boolean(),
    NEXT_PUBLIC_CELO_ENABLED: yup.boolean(),
    NEXT_PUBLIC_DEX_POOLS_ENABLED: yup.boolean()
      .when('NEXT_PUBLIC_CONTRACT_INFO_API_HOST', {
        is: (value: string) => Boolean(value),
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_DEX_POOLS_ENABLED can only be used with NEXT_PUBLIC_CONTRACT_INFO_API_HOST',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_ADDRESS_USERNAME_TAG: yup
      .mixed()
      .test('shape', 'Invalid schema were provided for NEXT_PUBLIC_ADDRESS_USERNAME_TAG, it should have api_url_template', (data) => {
        const isUndefined = data === undefined;
        const valueSchema = yup.object<AddressProfileAPIConfig>().transform(replaceQuotes).json().shape({
          api_url_template: yup.string().required(),
          tag_link_template: yup.string(),
          tag_icon: yup.string(),
          tag_bg_color: yup.string(),
          tag_text_color: yup.string(),
        });

        return isUndefined || valueSchema.isValidSync(data);
      }),
    NEXT_PUBLIC_XSTAR_SCORE_URL: yup.string().test(urlTest),
    NEXT_PUBLIC_GAME_BADGE_CLAIM_LINK: yup.string().test(urlTest),
    NEXT_PUBLIC_PUZZLE_GAME_BADGE_CLAIM_LINK: yup.string().test(urlTest),
    NEXT_PUBLIC_TX_EXTERNAL_TRANSACTIONS_CONFIG: yup.mixed().test(
      'shape',
      'Invalid schema were provided for NEXT_PUBLIC_TX_EXTERNAL_TRANSACTIONS_CONFIG, it should have chain_name, chain_logo_url, and explorer_url_template',
      (data) => {
        const isUndefined = data === undefined;
        const valueSchema = yup.object<TxExternalTxsConfig>().transform(replaceQuotes).json().shape({
          chain_name: yup.string().required(),
          chain_logo_url: yup.string().required(),
          explorer_url_template: yup.string().required(),
        });

        return isUndefined || valueSchema.isValidSync(data);
      }),
    NEXT_PUBLIC_FLASHBLOCKS_SOCKET_URL: yup.string().test(urlTest),
    NEXT_PUBLIC_HOT_CONTRACTS_ENABLED: yup.boolean(),

    // Misc
    NEXT_PUBLIC_USE_NEXT_JS_PROXY: yup.boolean(),
    NEXT_PUBLIC_API_KEYS_ALERT_MESSAGE: yup.string(),
    NEXT_PUBLIC_API_DOCS_ALERT_MESSAGE: yup.string(),
  })
  .concat(apisSchema)
  .concat(chainSchema)
  .concat(metaSchema)
  .concat(uiSchemas.homepageSchema)
  .concat(uiSchemas.navigationSchema)
  .concat(uiSchemas.footerSchema)
  .concat(uiSchemas.miscSchema)
  .concat(uiSchemas.viewsSchema)
  .concat(featuresSchemas.accountSchema)
  .concat(featuresSchemas.address3rdPartyWidgetsConfigSchema)
  .concat(featuresSchemas.adsSchema)
  .concat(featuresSchemas.apiDocsSchema)
  .concat(featuresSchemas.beaconChainSchema)
  .concat(featuresSchemas.bridgedTokensSchema)
  .concat(featuresSchemas.crossChainTxsSchema)
  .concat(featuresSchemas.defiDropdownSchema)
  .concat(featuresSchemas.highlightsConfigSchema)
  .concat(featuresSchemas.marketplaceSchema)
  .concat(featuresSchemas.megaEthSchema)
  .concat(featuresSchemas.multichainButtonSchema)
  .concat(featuresSchemas.nameServicesSchema)
  .concat(featuresSchemas.rollupSchema)
  .concat(featuresSchemas.tacSchema)
  .concat(featuresSchemas.userOpsSchema)
  .concat(featuresSchemas.zetaChainSchema)
  .concat(servicesSchema)

export default schema;
