import type { NextPage } from 'next';
import React from 'react';

import PageNextJs from 'nextjs/PageNextJs';

import Login from 'ui/pages/Login';

const Page: NextPage = () => {
  return (
    <PageNextJs pathname="/login">
      <Login/>
    </PageNextJs>
  );
};

export default Page;

export { login as getServerSideProps } from 'nextjs/getServerSideProps/main';
