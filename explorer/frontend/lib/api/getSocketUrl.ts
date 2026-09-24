import appConfig from 'configs/app';

export default function getSocketUrl(config: typeof appConfig = appConfig) {
  return config.apis.general ? `${ config.apis.general.socketEndpoint }${ config.apis.general.basePath ?? '' }/socket/v2` : undefined;
}
