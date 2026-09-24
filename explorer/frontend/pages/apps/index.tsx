import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const Marketplace = dynamic(() => import('ui/pages/Marketplace'), { ssr: false });

const Page: NextPage = () => (
  <PageNextJs pathname="/apps">
    <Marketplace/>
  </PageNextJs>
);

export default Page;

export { marketplace as getServerSideProps } from 'nextjs/getServerSideProps/main';
