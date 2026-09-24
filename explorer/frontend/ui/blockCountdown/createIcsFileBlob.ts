import { route } from 'nextjs/routes';

import config from 'configs/app';
import type { TMultichainContext } from 'lib/contexts/multichain';
import type dayjs from 'lib/date/dayjs';

interface Params {
  date: dayjs.Dayjs;
  blockHeight: string;
  multichainContext?: TMultichainContext | null;
}

const DATE_FORMAT = 'YYYYMMDDTHHmmss';

export default function createIcsFileBlob({ date, blockHeight, multichainContext }: Params): Blob {
  const chainConfig = multichainContext?.chain.app_config || config;

  const name = `Block #${ blockHeight } reminder | ${ chainConfig.chain.name }`;
  const description = `#${ blockHeight } block creation time on ${ chainConfig.chain.name } blockchain.`;
  const startTime = date.format(DATE_FORMAT);
  const endTime = date.add(15, 'minutes').format(DATE_FORMAT);
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const blockUrl = config.app.baseUrl + route({ pathname: '/block/[height_or_hash]', query: { height_or_hash: blockHeight } }, multichainContext);

  const icsContent = `BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
SUMMARY:${ name }
DESCRIPTION:${ description }
DTSTART;TZID=${ timeZone }:${ startTime }
DTEND;TZID=${ timeZone }:${ endTime }
URL:${ blockUrl }
END:VEVENT
END:VCALENDAR`;

  const blob = new Blob([ icsContent ], { type: 'text/calendar' });

  return blob;
}
