import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const Cluster = dynamic(() => import('ui/pages/Cluster'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/name-services/clusters/[name]" query={ props.query }>
      <Cluster/>
    </PageNextJs>
  );
};

export default Page;

export { nameServiceClusters as getServerSideProps } from 'nextjs/getServerSideProps/main';
