import type { NextRequest, NextResponse } from 'next/server';

import * as cookiesLib from 'lib/cookies';

const APP_PROFILE_HEADER = 'x-app-profile';
const APP_PROFILE_QUERY_PARAM = 'app-profile';
const PRIVATE_PROFILE_VALUE = 'private';

export default function appProfileMiddleware(req: NextRequest, res: NextResponse) {
  const headerValue = req.headers.get(APP_PROFILE_HEADER);
  // for testing purposes
  const queryValue = req.nextUrl.searchParams.get(APP_PROFILE_QUERY_PARAM);

  const profileValue = headerValue || queryValue;
  if (profileValue === PRIVATE_PROFILE_VALUE) {
    res.cookies.set(cookiesLib.NAMES.APP_PROFILE, PRIVATE_PROFILE_VALUE, cookiesLib.getDefaultAttributes());
  } else {
    res.cookies.delete({
      name: cookiesLib.NAMES.APP_PROFILE,
      ...cookiesLib.getDefaultAttributes(),
    });
  }
}
