import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const ApiDocs = dynamic(() => import('ui/pages/ApiDocs'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/api-docs">
      <ApiDocs/>
    </PageNextJs>
  );
};

export default Page;

export { apiDocs as getServerSideProps } from 'nextjs/getServerSideProps/main';
