import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const Pools = dynamic(() => import('ui/pages/Pools'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/pools">
      <Pools/>
    </PageNextJs>
  );
};

export default Page;

export { pools as getServerSideProps } from 'nextjs/getServerSideProps/main';
