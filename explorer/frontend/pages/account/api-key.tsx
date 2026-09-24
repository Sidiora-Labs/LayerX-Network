import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const ApiKeys = dynamic(() => import('ui/pages/ApiKeys'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/account/api-key">
      <ApiKeys/>
    </PageNextJs>
  );
};

export default Page;

export { account as getServerSideProps } from 'nextjs/getServerSideProps/main';
