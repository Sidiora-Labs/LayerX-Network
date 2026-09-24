import mixpanel from 'mixpanel-browser';

import config from 'configs/app';

import type { EventTypes, EventPayload } from './utils';

type TrackFnArgs = Parameters<typeof mixpanel.track>;

export default function logEvent<EventType extends EventTypes>(
  type: EventType,
  properties?: EventPayload<EventType>,
  optionsOrCallback?: TrackFnArgs[2],
  callback?: TrackFnArgs[3],
) {
  if (!config.features.mixpanel.isEnabled) {
    return;
  }
  mixpanel.track(type, properties, optionsOrCallback, callback);
}
