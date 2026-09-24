/* eslint-disable no-restricted-imports */
import type { MountOptions } from '@playwright/experimental-ct-react';
import type { Locator, TestFixture } from '@playwright/test';
import type router from 'next/router';
import React from 'react';

import type { Props as TestAppProps } from 'playwright/TestApp';
import TestApp from 'playwright/TestApp';

interface MountResult extends Locator {
  unmount(): Promise<void>;
  update(component: React.JSX.Element): Promise<void>;
}

interface AppHooksConfig {
  router: Partial<Pick<typeof router, 'query' | 'isReady' | 'asPath' | 'pathname'>>;
}

type Mount = <HooksConfig extends AppHooksConfig>(component: React.JSX.Element, options?: MountOptions<HooksConfig>) => Promise<MountResult>;

export type RenderFixture = (
  component: React.JSX.Element,
  options?: MountOptions<AppHooksConfig>,
  props?: Omit<TestAppProps, 'children'>
) => Promise<MountResult>;

const fixture: TestFixture<RenderFixture, { mount: Mount }> = async({ mount }, use) => {
  await use((component, options, props) => {
    return mount(
      <TestApp { ...props }>{ component }</TestApp>,
      options,
    );
  });
};

export default fixture;
