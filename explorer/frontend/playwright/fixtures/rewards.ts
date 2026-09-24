import type { BrowserContext, TestFixture } from '@playwright/test';

import config from 'configs/app';
import * as cookies from 'lib/cookies';

// This JWT token contains 0xd789a607CEac2f0E14867de4EB15b15C9FFB5859 address
const token = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIweGQ3ODlhNjA3Q0VhYzJmMEUxNDg2N2RlNEVCMTViMTVDOUZGQjU4NTkiLCJpYXQiOjE3MzA0NzAyNTIsImV4cCI6MTczMDQ3MDU1Mn0.uhWH59mJQhpWcK8RHaLQ-X_nieXZsYE-VdcPrjYNvp4'; // eslint-disable-line max-len

export function authenticateUser(context: BrowserContext) {
  context.addCookies([ { name: cookies.NAMES.REWARDS_API_TOKEN, value: token, domain: config.app.host, path: '/' } ]);
}

export const contextWithRewards: TestFixture<BrowserContext, { context: BrowserContext }> = async({ context }, use) => {
  authenticateUser(context);
  use(context);
};
