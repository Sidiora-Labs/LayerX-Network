import type { NextPage } from 'next';
import dynamic from 'next/dynamic';
import React from 'react';

import type { Props } from 'nextjs/getServerSideProps/handlers';
import PageNextJs from 'nextjs/PageNextJs';

import config from 'configs/app';

const validatorsFeature = config.features.validators;

const ValidatorDetails = dynamic(() => {
  if (validatorsFeature.isEnabled && validatorsFeature.chainType === 'zilliqa') {
    return import('ui/pages/ValidatorZilliqa');
  }

  throw new Error('Validators feature is not enabled.');
}, { ssr: false });

const Page: NextPage<Props> = (props) => {
  return (
    <PageNextJs pathname="/validators/[id]" query={ props.query }>
      <ValidatorDetails/>
    </PageNextJs>
  );
};

export default Page;

export { validatorDetails as getServerSideProps } from 'nextjs/getServerSideProps/main';
