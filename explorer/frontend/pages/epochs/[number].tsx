import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const Epoch = dynamic(() => import('ui/pages/Epoch'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/epochs/[number]" query={ props.query }>
      <Epoch/>
    </PageNextJs>
  );
};

export default Page;

export { celo as getServerSideProps } from 'nextjs/getServerSideProps/main';
