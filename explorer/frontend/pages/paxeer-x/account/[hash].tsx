import type { GetServerSideProps, NextPage } from 'next';
import React from 'react';

import type { Route } from 'nextjs-routes';
import type { Props } from 'nextjs/getServerSideProps/handlers';
import * as gSSP from 'nextjs/getServerSideProps/main';
import PageNextJs from 'nextjs/PageNextJs';

import PaxeerXAccount from 'ui/pages/PaxeerXAccount';

const pathname: Route['pathname'] = '/paxeer-x/account/[hash]';

const Page: NextPage<Props<typeof pathname>> = (props: Props<typeof pathname>) => {
  return (
    <PageNextJs pathname={ pathname } query={ props.query }>
      <PaxeerXAccount/>
    </PageNextJs>
  );
};

export default Page;

export const getServerSideProps: GetServerSideProps<Props<typeof pathname>> = gSSP.paxeerXLists;
