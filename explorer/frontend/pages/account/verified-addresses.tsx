import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const VerifiedAddresses = dynamic(() => import('ui/pages/VerifiedAddresses'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/account/verified-addresses">
      <VerifiedAddresses/>
    </PageNextJs>
  );
};

export default Page;

export { verifiedAddresses as getServerSideProps } from 'nextjs/getServerSideProps/main';
