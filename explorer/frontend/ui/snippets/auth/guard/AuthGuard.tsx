import dynamic from 'next/dynamic';

import config from 'configs/app';

const AuthGuardAuth0 = dynamic(() => import('./AuthGuardAuth0'), { ssr: false });
const AuthGuardDynamic = dynamic(() => import('./AuthGuardDynamic'), { ssr: false });

const feature = config.features.account;

const AuthGuard = (() => {
  if (feature.isEnabled && feature.authProvider === 'dynamic') {
    return AuthGuardDynamic;
  }

  return AuthGuardAuth0;
})();

export default AuthGuard;
