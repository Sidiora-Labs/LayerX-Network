import type CspDev from 'csp-dev';

import config from 'configs/app';

export function googleReCaptcha(): CspDev.DirectiveDescriptor {
  if (!config.services.reCaptchaV2.siteKey) {
    return {};
  }

  return {
    'connect-src': [
      'https://www.google.com/recaptcha/api2/clr',
    ],
    'script-src': [
      'https://www.google.com/recaptcha/api.js',
      'https://www.gstatic.com',
      'https://translate.google.com',
      '\'sha256-FDyPg8CqqIpPAfGVKx1YeKduyLs0ghNYWII21wL+7HM=\'',
    ],
    'style-src': [
      'https://www.gstatic.com',
    ],
    'img-src': [
      'https://translate.google.com',
      'https://www.gstatic.com',
    ],
    'frame-src': [
      'https://www.google.com/recaptcha/api2/anchor',
      'https://www.google.com/recaptcha/api2/bframe',
    ],
  };
}
