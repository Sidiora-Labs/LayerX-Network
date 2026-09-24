import type { NextRequest } from 'next/server';
import { NextResponse } from 'next/server';

import * as csp from 'nextjs/csp/index';
import * as middlewares from 'nextjs/middlewares/index';

export async function middleware(req: NextRequest) {
  const isPageRequest = req.headers.get('accept')?.includes('text/html');
  const start = Date.now();

  if (!isPageRequest) {
    return;
  }

  const accountResponse = middlewares.account(req);
  if (accountResponse) {
    return accountResponse;
  }

  const res = NextResponse.next();

  middlewares.appProfile(req, res);
  middlewares.colorTheme(req, res);
  middlewares.addressFormat(req, res);
  middlewares.scamTokens(req, res);
  middlewares.poorReputationTokens(req, res);

  const end = Date.now();

  const cspHeader = await csp.get(req);

  res.headers.append('Content-Security-Policy', cspHeader);
  res.headers.append('Server-Timing', `middleware;dur=${ end - start }`);
  res.headers.append('Docker-ID', process.env.HOSTNAME || '');

  return res;
}

/**
 * Configure which routes should pass through the Middleware.
 */
export const config = {
  matcher: [ '/', '/:notunderscore((?!_next).+)' ],
  // matcher: [
  //   '/((?!.*\\.|api\\/|node-api\\/).*)', // exclude all static + api + node-api routes
  // ],
};
