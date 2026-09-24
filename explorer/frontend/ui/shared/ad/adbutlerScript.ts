/* eslint-disable max-len */

import config from 'configs/app';

export const ADBUTLER_ACCOUNT = 182226;

export const connectAdbutler = `if (!window.AdButler){(function(){var s = document.createElement("script"); s.async = true; s.type = "text/javascript";s.src = 'https://servedbyadbutler.com/app.js';var n = document.getElementsByTagName("script")[0]; n.parentNode.insertBefore(s, n);}());}`;

export const placeAd = ((isMobile: boolean | undefined) => {
  const feature = config.features.adsBanner;

  if (!('adButler' in feature)) {
    return;
  }

  if (isMobile) {
    return `
      var AdButler = AdButler || {}; AdButler.ads = AdButler.ads || [];
      var abkw = window.abkw || '';
      var plc${ feature.adButler.config.mobile.id } = window.plc${ feature.adButler.config.mobile.id } || 0;
      document.getElementById('ad-banner').innerHTML = '<'+'div id="placement_${ feature.adButler.config.mobile.id }_'+plc${ feature.adButler.config.mobile.id }+'"></'+'div>';
      AdButler.ads.push({handler: function(opt){ AdButler.register(${ ADBUTLER_ACCOUNT }, ${ feature.adButler.config.mobile.id }, [${ feature.adButler.config.mobile.width },${ feature.adButler.config.mobile.height }], 'placement_${ feature.adButler.config.mobile.id }_'+opt.place, opt); }, opt: { place: plc${ feature.adButler.config.mobile.id }++, keywords: abkw, domain: 'servedbyadbutler.com', click:'CLICK_MACRO_PLACEHOLDER' }});
    `;
  }

  return `
    var AdButler = AdButler || {}; AdButler.ads = AdButler.ads || [];
    var abkw = window.abkw || '';
    const isMobile = window.matchMedia("only screen and (max-width: 1000px)").matches;
    if (isMobile) {
        var plc${ feature.adButler.config.mobile.id } = window.plc${ feature.adButler.config.mobile.id } || 0;
        document.getElementById('ad-banner').innerHTML = '<'+'div id="placement_${ feature.adButler.config.mobile.id }_'+plc${ feature.adButler.config.mobile.id }+'"></'+'div>';
        AdButler.ads.push({handler: function(opt){ AdButler.register(${ ADBUTLER_ACCOUNT }, ${ feature.adButler.config.mobile.id }, [${ feature.adButler.config.mobile.width },${ feature.adButler.config.mobile.height }], 'placement_${ feature.adButler.config.mobile.id }_'+opt.place, opt); }, opt: { place: plc${ feature.adButler.config.mobile.id }++, keywords: abkw, domain: 'servedbyadbutler.com', click:'CLICK_MACRO_PLACEHOLDER' }});
    } else {
        var plc${ feature.adButler.config.desktop.id } = window.plc${ feature.adButler.config.desktop.id } || 0;
        document.getElementById('ad-banner').innerHTML = '<'+'div id="placement_${ feature.adButler.config.desktop.id }_'+plc${ feature.adButler.config.desktop.id }+'"></'+'div>';
        AdButler.ads.push({handler: function(opt){ AdButler.register(${ ADBUTLER_ACCOUNT }, ${ feature.adButler.config.desktop.id }, [${ feature.adButler.config.desktop.width },${ feature.adButler.config.desktop.height }], 'placement_${ feature.adButler.config.desktop.id }_'+opt.place, opt); }, opt: { place: plc${ feature.adButler.config.desktop.id }++, keywords: abkw, domain: 'servedbyadbutler.com', click:'CLICK_MACRO_PLACEHOLDER' }});
    }
  `;
});
