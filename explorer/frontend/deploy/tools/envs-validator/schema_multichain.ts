declare module 'yup' {
    interface StringSchema {
      // Yup's URL validator is not perfect so we made our own
      // https://github.com/jquense/yup/pull/1859
      url(): never;
    }
  }
  
import * as yup from 'yup';
import { urlTest, protocols } from './utils';
import * as uiSchemas from './schemas/ui';
import * as featuresSchemas from './schemas/features';
import servicesSchemas from './schemas/services';
import { replaceQuotes } from 'configs/app/utils';
import { IDENTICON_TYPES } from 'types/views/address';

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
    // 1. App configuration
    NEXT_PUBLIC_APP_HOST: yup.string().required(),
    NEXT_PUBLIC_APP_PROTOCOL: yup.string().oneOf(protocols),
    NEXT_PUBLIC_APP_PORT: yup.number().positive().integer(),
    NEXT_PUBLIC_APP_ENV: yup.string(),
    NEXT_PUBLIC_APP_INSTANCE: yup.string(),

    // 2. Blockchain parameters
    NEXT_PUBLIC_NETWORK_NAME: yup.string().required(),
    NEXT_PUBLIC_NETWORK_SHORT_NAME: yup.string(),
    NEXT_PUBLIC_IS_TESTNET: yup.boolean(),

    // 3. UI views configuration
    // Some settings that we actually support in multichain mode
    NEXT_PUBLIC_VIEWS_ADDRESS_IDENTICON_TYPE: yup.string().oneOf(IDENTICON_TYPES),

    // 5. Features configuration
    // NOTE!: Not all features are supported in multichain mode, and some of them not relevant or enabled per chain basis
    // Below listed supported features and the features that are enabled by default, so we have to turn them off
    NEXT_PUBLIC_OG_DESCRIPTION: yup.string(),
    NEXT_PUBLIC_OG_IMAGE_URL: yup.string().test(urlTest),

    NEXT_PUBLIC_GAS_TRACKER_ENABLED: yup.boolean().equals([false]),
    NEXT_PUBLIC_ADVANCED_FILTER_ENABLED: yup.boolean().equals([false]),
    NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED: yup.boolean().equals([false]),
    NEXT_PUBLIC_API_DOCS_TABS: yup.array().transform(replaceQuotes).json().max(0),

    // 6. Multichain configuration
    NEXT_PUBLIC_MULTICHAIN_ENABLED: yup.boolean(),
    NEXT_PUBLIC_MULTICHAIN_CLUSTER: yup.string(),
    NEXT_PUBLIC_MULTICHAIN_AGGREGATOR_API_HOST: yup.string().test(urlTest),
    NEXT_PUBLIC_MULTICHAIN_STATS_API_HOST: yup.string().test(urlTest),

    // Misc
    NEXT_PUBLIC_USE_NEXT_JS_PROXY: yup.boolean(),
  })
  .concat(uiSchemas.homepageSchema)
  .concat(uiSchemas.navigationSchema)
  .concat(uiSchemas.footerSchema)
  .concat(uiSchemas.miscSchema)
  .concat(featuresSchemas.adsSchema)
  .concat(featuresSchemas.defiDropdownSchema)
  .concat(featuresSchemas.multichainButtonSchema)
  .concat(featuresSchemas.userOpsSchema)
  .concat(servicesSchemas);

export default schema;
