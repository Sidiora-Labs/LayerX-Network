import * as yup from 'yup';
import { replaceQuotes } from 'configs/app/utils';
import type { AdBannerProviders, AdBannerAdditionalProviders, AdTextProviders } from 'types/client/adProviders';
import type { AdButlerConfig } from 'types/client/adButlerConfig';
import { SUPPORTED_AD_TEXT_PROVIDERS, SUPPORTED_AD_BANNER_PROVIDERS, SUPPORTED_AD_BANNER_ADDITIONAL_PROVIDERS } from 'types/client/adProviders';

const adButlerConfigSchema = yup
  .object<AdButlerConfig>()
  .transform(replaceQuotes)
  .json()
  .when('NEXT_PUBLIC_AD_BANNER_PROVIDER', {
    is: (value: AdBannerProviders) => value === 'adbutler',
    then: (schema) => schema
      .shape({
        id: yup.string().required(),
        width: yup.number().positive().required(),
        height: yup.number().positive().required(),
      })
      .required(),
  })
  .when('NEXT_PUBLIC_AD_BANNER_ADDITIONAL_PROVIDER', {
    is: (value: AdBannerProviders) => value === 'adbutler',
    then: (schema) => schema
      .shape({
        id: yup.string().required(),
        width: yup.number().positive().required(),
        height: yup.number().positive().required(),
      })
      .required(),
  });

export const adsSchema = yup.object({
    NEXT_PUBLIC_AD_TEXT_PROVIDER: yup.string<AdTextProviders>().oneOf(SUPPORTED_AD_TEXT_PROVIDERS),
    NEXT_PUBLIC_AD_BANNER_PROVIDER: yup.string<AdBannerProviders>().oneOf(SUPPORTED_AD_BANNER_PROVIDERS),
    NEXT_PUBLIC_AD_BANNER_ADDITIONAL_PROVIDER: yup.string<AdBannerAdditionalProviders>().oneOf(SUPPORTED_AD_BANNER_ADDITIONAL_PROVIDERS),
    NEXT_PUBLIC_AD_ADBUTLER_CONFIG_DESKTOP: adButlerConfigSchema,
    NEXT_PUBLIC_AD_ADBUTLER_CONFIG_MOBILE: adButlerConfigSchema,
    NEXT_PUBLIC_AD_BANNER_ENABLE_SPECIFY: yup.boolean(),
});