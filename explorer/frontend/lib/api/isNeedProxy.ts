import config from 'configs/app';

export default function isNeedProxy() {
  if (config.app.useProxy) {
    return true;
  }

  // ONLY FOR DEV OR REVIEW ENVIRONMENTS (NOT PRODUCTION)
  // because we have some resources that require credentials, we need to use the proxy
  // otherwise the cross-origin requests with "credentials: include" will be blocked
  return config.app.isDev || config.app.isReview;
}
