import type { TestFixture, Page } from '@playwright/test';

import * as textAdMock from 'mocks/ad/textAd';

export type MockTextAdFixture = () => Promise<void>;

const SEVIO_LOADER_URL = 'https://cdn.adx.ws/scripts/loader.js';

const fixture: TestFixture<MockTextAdFixture, { page: Page }> = async({ page }, use) => {
  await use(async() => {
    const { thumbnail, sponsored, title, clickURL, ctatext } = textAdMock.duck;

    const adHtml = [
      `<strong>Ads:</strong> `,
      // eslint-disable-next-line max-len
      `<img src="${ thumbnail }" width="20" height="20" style="display: inline-block; vertical-align: text-bottom; margin-right: 4px; margin-left: 7px;" alt="" />`,
      `${ sponsored } \u2013 ${ title } `,
      `<a href="${ clickURL }" target="_blank">${ ctatext }</a>`,
    ].join('');

    // Route the Sevio loader script so that onLoad fires (not onError), which starts
    // the ad-served timeout in the component.
    await page.route(SEVIO_LOADER_URL, (route) => {
      return route.fulfill({ status: 200, contentType: 'application/javascript', body: '' });
    });

    await page.evaluate((html) => {
      const queue: Array<Array<Record<string, string>>> = [];
      queue.push = () => {
        // Defer injection so the MutationObserver (set up in the next useEffect) is
        // already observing the div when the HTML arrives.
        setTimeout(() => {
          const container = document.querySelector('.sevioads');
          if (container) {
            container.innerHTML = html;
          }
        }, 0);
        return queue.length;
      };
      Object.defineProperty(window, 'sevioads', {
        value: queue,
        writable: true,
        configurable: true,
      });
    }, adHtml);

    await page.route(thumbnail, (route) => {
      return route.fulfill({
        status: 200,
        path: './playwright/mocks/image_s.jpg',
      });
    });
  });
};

export default fixture;
