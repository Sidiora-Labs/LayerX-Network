import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const Blob = dynamic(() => import('ui/pages/Blob'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/blobs/[hash]" query={ props.query }>
      <Blob/>
    </PageNextJs>
  );
};

export default Page;

export { dataAvailability as getServerSideProps } from 'nextjs/getServerSideProps/main';
