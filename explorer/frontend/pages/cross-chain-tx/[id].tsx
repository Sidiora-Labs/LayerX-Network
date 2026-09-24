import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

const TxCrossChain = dynamic(() => import('ui/crossChain/tx/TxCrossChain'), { ssr: false });

const Page: NextPage<Props> = (props: Props) => {
  return (
    <PageNextJs pathname="/cross-chain-tx/[id]" query={ props.query }>
      <TxCrossChain/>
    </PageNextJs>
  );
};

export default Page;

export { crossChainTxs as getServerSideProps } from 'nextjs/getServerSideProps/main';
