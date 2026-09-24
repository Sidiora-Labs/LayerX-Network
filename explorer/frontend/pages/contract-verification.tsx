import type { NextPage } from 'next';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

import ContractVerification from 'ui/pages/ContractVerification';

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/contract-verification" query={ props.query }>
      <ContractVerification/>
    </PageNextJs>
  );
};

export default Page;

export { base as getServerSideProps } from 'nextjs/getServerSideProps/main';
