import config from 'configs/app';

const feature = config.features.account;

const useLinkEmail = (feature.isEnabled && feature.authProvider === 'dynamic') ?
  (await import('./useLinkEmailDynamic')).default :
  (await import('./useLinkEmailFallback')).default;

export default useLinkEmail;
