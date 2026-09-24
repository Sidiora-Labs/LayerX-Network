import './fonts.css';
import './index.css';
import '../nextjs/global.css';
import { beforeMount } from '@playwright/experimental-ct-react/hooks';
import MockDate from 'mockdate';
import * as router from 'next/router';

const NEXT_ROUTER_MOCK = {
  query: {},
  pathname: '',
  push: () => Promise.resolve(),
  replace: () => Promise.resolve(),
};

beforeMount(async({ hooksConfig }: { hooksConfig?: { router: typeof router } }) => {
  // Before mount, redefine useRouter to return mock value from test.

  // @ts-ignore: I really want to redefine this property :)
  // eslint-disable-next-line no-import-assign
  router.useRouter = () => ({
    ...NEXT_ROUTER_MOCK,
    ...hooksConfig?.router,
  });

  // set current date
  MockDate.set('2022-11-11T12:00:00Z');
});

export {};
