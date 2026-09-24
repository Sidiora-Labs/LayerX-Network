import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import { MultichainProvider } from 'lib/contexts/multichain';
import CsvExport from 'ui/pages/CsvExport';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/chain/[chain_slug]/csv-export">
      <MultichainProvider>
        <CsvExport/>
      </MultichainProvider>
    </PageNextJs>
  );
};

export default Page;

export { csvExport as getServerSideProps } from 'nextjs/getServerSideProps/multichain';
