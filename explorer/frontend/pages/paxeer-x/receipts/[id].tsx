import type { GetServerSideProps, NextPage } from 'next';
import React from 'react';

import type { Route } from 'nextjs-routes';
import type { Props } from 'nextjs/getServerSideProps/handlers';
import * as gSSP from 'nextjs/getServerSideProps/main';
import PageNextJs from 'nextjs/PageNextJs';

import PaxeerXReceipt from 'ui/pages/PaxeerXReceipt';

const pathname: Route['pathname'] = '/paxeer-x/receipts/[id]';

const Page: NextPage<Props<typeof pathname>> = (props: Props<typeof pathname>) => {
  return (
    <PageNextJs pathname={ pathname } query={ props.query }>
      <PaxeerXReceipt/>
    </PageNextJs>
  );
};

export default Page;

export const getServerSideProps: GetServerSideProps<Props<typeof pathname>> = gSSP.paxeerXLists;
