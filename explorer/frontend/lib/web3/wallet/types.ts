import type * as mixpanel from 'lib/mixpanel/index';

export interface Params {
  source: mixpanel.EventPayload<mixpanel.EventTypes.WALLET_CONNECT>['Source'];
  onConnect?: () => void;
}

export interface Result {
  connect: () => void;
  disconnect: () => void;
  isOpen: boolean;
  isConnected: boolean;
  isReconnecting: boolean;
  address: string | undefined;
  openModal: () => void;
  type?: 'dynamicwaas';
}
