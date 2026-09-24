import type { BrowserContext } from '@playwright/test';
import React from 'react';

import * as rewardsBalanceMock from 'mocks/rewards/balance';
import * as dailyRewardMock from 'mocks/rewards/dailyReward';
import * as profileMock from 'mocks/user/profile';
import { contextWithAuth } from 'playwright/fixtures/auth';
import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { contextWithRewards } from 'playwright/fixtures/rewards';
import { test, expect } from 'playwright/lib';
import * as pwConfig from 'playwright/utils/config';

import HeroBanner from './HeroBanner';

const authTest = test.extend<{ context: BrowserContext }>({
  context: contextWithAuth,
}).extend<{ context: BrowserContext }>({
  context: contextWithRewards,
});

authTest('customization +@dark-mode', async({ render, page, mockEnvs, mockApiResponse }) => {
  const IMAGE_URL = 'https://localhost:3000/my-image.png';

  await mockEnvs([
    ...ENVS_MAP.rewardsService,
    // eslint-disable-next-line max-len
    [ 'NEXT_PUBLIC_HOMEPAGE_HERO_BANNER_CONFIG', `{"background":["lightpink","no-repeat center/cover url(${ IMAGE_URL })"],"text_color":["deepskyblue","white"],"border":["3px solid green","3px dashed yellow"],"button":{"_default":{"background":["deeppink"],"text_color":["white"]},"_selected":{"background":["lime"]}}}` ],
  ]);

  await page.route(IMAGE_URL, (route) => {
    return route.fulfill({
      status: 200,
      path: './playwright/mocks/image_long.jpg',
    });
  });

  await mockApiResponse('general:user_info', profileMock.withEmailAndWallet);
  await mockApiResponse('rewards:user_balances', rewardsBalanceMock.base);
  await mockApiResponse('rewards:user_daily_check', dailyRewardMock.base);

  const component = await render(<HeroBanner/>);

  await expect(component).toHaveScreenshot({
    mask: [ page.locator(pwConfig.adsBannerSelector) ],
    maskColor: pwConfig.maskColor,
  });
});
