import * as yup from 'yup';
import type { MarketplaceAppBase, MarketplaceAppSocialInfo, EssentialDappsConfig, MarketplaceTitles } from 'types/client/marketplace';
import { urlTest } from '../../utils';
import { replaceQuotes } from 'configs/app/utils';

const marketplaceAppSchema: yup.ObjectSchema<MarketplaceAppBase & MarketplaceAppSocialInfo> = yup
  .object({
    id: yup.string().required(),
    external: yup.boolean(),
    title: yup.string().required(),
    logo: yup.string().test(urlTest).required(),
    logoDarkMode: yup.string().test(urlTest),
    shortDescription: yup.string().required(),
    categories: yup.array().of(yup.string().required()).required(),
    url: yup.string().test(urlTest).required(),
    author: yup.string().required(),
    description: yup.string().required(),
    site: yup.string().test(urlTest),
    twitter: yup.string().test(urlTest),
    telegram: yup.string().test(urlTest),
    github: yup.lazy(value =>
      Array.isArray(value) ?
        yup.array().of(yup.string().required().test(urlTest)) :
        yup.string().test(urlTest),
    ),
    discord: yup.string().test(urlTest),
    internalWallet: yup.boolean(),
    priority: yup.number(),
  });

export const marketplaceSchema = yup
  .object()
  .shape({
    NEXT_PUBLIC_MARKETPLACE_ENABLED: yup.boolean(),
    NEXT_PUBLIC_MARKETPLACE_CONFIG_URL: yup
      .array()
      .json()
      .of(marketplaceAppSchema)
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema,
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_CONFIG_URL cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_CATEGORIES_URL: yup
      .array()
      .json()
      .of(yup.string())
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema,
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_CATEGORIES_URL cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_SUBMIT_FORM: yup
      .string()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema.test(urlTest).required(),
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_SUBMIT_FORM cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_SUGGEST_IDEAS_FORM: yup
      .string()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema.test(urlTest),
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_SUGGEST_IDEAS_FORM cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_FEATURED_APP: yup
      .string()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema,
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_FEATURED_APP cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_BANNER_CONTENT_URL: yup
      .string()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema.test(urlTest),
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_BANNER_CONTENT_URL cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_BANNER_LINK_URL: yup
      .string()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema.test(urlTest),
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_BANNER_LINK_URL cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_GRAPH_LINKS_URL: yup
      .string()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema,
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.max(-1, 'NEXT_PUBLIC_MARKETPLACE_GRAPH_LINKS_URL cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED'),
      }),
    NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG: yup
      .mixed()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema.test('shape', 'Invalid schema were provided for NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG, it should contain optional swap/revoke/multisend sections with required fields', (data) => {
          const isUndefined = data === undefined;
          const chainsSchema = yup.array().of(yup.string().required()).min(1).required();
          const valueSchema = yup.object<EssentialDappsConfig>().transform(replaceQuotes).json().shape({
            swap: yup.lazy(value => value ?
              yup.object<EssentialDappsConfig['swap']>().shape({
                url: yup.string().test(urlTest).required(),
                chains: chainsSchema,
                fee: yup.string().required(),
                integrator: yup.string().required(),
              }) :
              yup.object().nullable(),
            ),
            revoke: yup.lazy(value => value ?
              yup.object<EssentialDappsConfig['revoke']>().shape({ chains: chainsSchema }) :
              yup.object().nullable(),
            ),
            multisend: yup.lazy(value => value ?
              yup.object<EssentialDappsConfig['multisend']>().shape({
                chains: chainsSchema,
                posthogKey: yup.string(),
                posthogHost: yup.string().test(urlTest),
              }) :
              yup.object().nullable(),
            ),
          });
          return isUndefined || valueSchema.isValidSync(data);
        }),
        // eslint-disable-next-line max-len
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_MARKETPLACE_TITLES: yup
      .mixed()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema.test('shape', 'Invalid schema were provided for NEXT_PUBLIC_MARKETPLACE_TITLES', (data) => {
          const isUndefined = data === undefined;
          const valueSchema = yup.object<MarketplaceTitles>().transform(replaceQuotes).json().shape({
            menu_item: yup.string(),
            title: yup.string(),
            subtitle_essential_dapps: yup.string(),
            subtitle_list: yup.string(),
          });

          return isUndefined || valueSchema.isValidSync(data);
        }),
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_MARKETPLACE_TITLES cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED',
          value => value === undefined,
        ),
      }),
    NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_AD_ENABLED: yup
      .boolean()
      .when('NEXT_PUBLIC_MARKETPLACE_ENABLED', {
        is: true,
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_AD_ENABLED cannot not be used without NEXT_PUBLIC_MARKETPLACE_ENABLED',
          value => value === undefined,
        ),
      }),
  });
