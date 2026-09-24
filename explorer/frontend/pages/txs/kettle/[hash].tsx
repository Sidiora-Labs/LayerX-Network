import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const KettleTxs = dynamic(() => import('ui/pages/KettleTxs'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/txs/kettle/[hash]" query={ props.query }>
      <KettleTxs/>
    </PageNextJs>
  );
};

export default Page;

export { suave as getServerSideProps } from 'nextjs/getServerSideProps/main';
