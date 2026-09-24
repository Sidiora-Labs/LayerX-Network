import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

const AccountsLabelSearch = dynamic(() => import('ui/pages/AccountsLabelSearch'), { ssr: false });

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/accounts/label/[slug]">
      <AccountsLabelSearch/>
    </PageNextJs>
  );
};

export default Page;

export { accountsLabelSearch as getServerSideProps } from 'nextjs/getServerSideProps/main';
