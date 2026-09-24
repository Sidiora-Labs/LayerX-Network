import mixpanel from 'mixpanel-browser';

import type { PickByType } from 'types/utils';

interface UserProfileProperties {
  'With Account': boolean;
  'With Connected Wallet': boolean;
  'Device Type': string;
  'First Time Join': string;
}

type UserProfilePropertiesNumerable = PickByType<UserProfileProperties, number>;

export function set(props: Partial<UserProfileProperties>) {
  mixpanel.people.set(props);
}

export function setOnce(props: Partial<UserProfileProperties>) {
  mixpanel.people.set_once(props);
}

export function increment(props: UserProfilePropertiesNumerable) {
  mixpanel.people.increment(props);
}
