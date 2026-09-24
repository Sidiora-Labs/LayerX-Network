import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import CsvExport from 'ui/pages/CsvExport';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/csv-export">
      <CsvExport/>
    </PageNextJs>
  );
};

export default Page;

export { csvExport as getServerSideProps } from 'nextjs/getServerSideProps/main';
