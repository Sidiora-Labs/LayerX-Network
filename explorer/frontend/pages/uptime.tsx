import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import Uptime from 'ui/megaEth/uptime/Uptime';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/uptime">
      <Uptime/>
    </PageNextJs>
  );
};

export default Page;

export { megaEth as getServerSideProps } from 'nextjs/getServerSideProps/main';
