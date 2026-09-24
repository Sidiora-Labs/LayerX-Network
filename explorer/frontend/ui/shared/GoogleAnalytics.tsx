import Script from 'next/script';
import React from 'react';

import config from 'configs/app';

const feature = config.features.googleAnalytics;

const GoogleAnalytics = () => {
  if (!feature.isEnabled) {
    return null;
  }

  const id = feature.propertyId;

  return (
    <>
      <Script strategy="lazyOnload" src={ `https://www.googletagmanager.com/gtag/js?id=${ id }` }/>
      <Script strategy="lazyOnload" id="google-analytics">
        { `
          window.dataLayer = window.dataLayer || [];
          function gtag(){dataLayer.push(arguments);}
          gtag('js', new Date());
          gtag('config', window.__envs.NEXT_PUBLIC_GOOGLE_ANALYTICS_PROPERTY_ID);
        ` }
      </Script>
    </>
  );
};

export default React.memo(GoogleAnalytics);
