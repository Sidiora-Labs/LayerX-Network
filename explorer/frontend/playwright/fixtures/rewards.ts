import type { BrowserContext, TestFixture } from '@playwright/test';

import config from 'configs/app';
import * as cookies from 'lib/cookies';

// The rewards API token is never verified in the browser: the rewards context decodes it and
// compares its "sub" claim with the address of the mocked user profile (mocks/user/profile).
// The token is therefore assembled here from plainly synthetic, unsigned parts.
export const REGISTERED_ADDRESS = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';

const UNSIGNED_SIGNATURE_PLACEHOLDER = 'not-a-signature';

function encodeSegment(segment: Record<string, unknown>) {
  return Buffer.from(JSON.stringify(segment)).toString('base64url');
}

export function buildRewardsApiToken(address = REGISTERED_ADDRESS) {
  const issuedAt = Math.floor(Date.now() / 1000);

  return [
    encodeSegment({ alg: 'none', typ: 'JWT' }),
    encodeSegment({ sub: address, iat: issuedAt, exp: issuedAt + 300 }),
    UNSIGNED_SIGNATURE_PLACEHOLDER,
  ].join('.');
}

export function authenticateUser(context: BrowserContext) {
  context.addCookies([ { name: cookies.NAMES.REWARDS_API_TOKEN, value: buildRewardsApiToken(), domain: config.app.host, path: '/' } ]);
}

export const contextWithRewards: TestFixture<BrowserContext, { context: BrowserContext }> = async({ context }, use) => {
  authenticateUser(context);
  use(context);
};
