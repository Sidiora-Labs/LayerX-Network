import { replaceQuotes } from 'configs/app/utils';
import * as yup from 'yup';

// External services envs
export default yup.object({
    NEXT_PUBLIC_WALLET_CONNECT_PROJECT_ID: yup.string(),
    NEXT_PUBLIC_WALLET_CONNECT_FEATURED_WALLET_IDS: yup.array().transform(replaceQuotes).json().of(yup.string()),

    NEXT_PUBLIC_RE_CAPTCHA_APP_SITE_KEY: yup.string(),
    NEXT_PUBLIC_GOOGLE_ANALYTICS_PROPERTY_ID: yup.string(),
    NEXT_PUBLIC_GROWTH_BOOK_CLIENT_KEY: yup.string(),
    NEXT_PUBLIC_ROLLBAR_CLIENT_TOKEN: yup.string(),

    // mixpanel
    NEXT_PUBLIC_MIXPANEL_PROJECT_TOKEN: yup.string(),
    NEXT_PUBLIC_MIXPANEL_CONFIG_OVERRIDES: yup
      .object()
      .transform(replaceQuotes)
      .json()
      .when('NEXT_PUBLIC_MIXPANEL_PROJECT_TOKEN', {
        is: (value: string) => Boolean(value),
        then: (schema) => schema,
        otherwise: (schema) => schema.test(
          'not-exist',
          'NEXT_PUBLIC_MIXPANEL_CONFIG_OVERRIDES can only be used if NEXT_PUBLIC_MIXPANEL_PROJECT_TOKEN is set to a non-empty string',
          value => value === undefined,
        ),
      }),
});