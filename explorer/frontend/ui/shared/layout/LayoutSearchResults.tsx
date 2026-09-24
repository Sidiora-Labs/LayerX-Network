import React from 'react';

import type { Props } from './types';

import * as Layout from './components';

const LayoutSearchResults = ({ children }: Props) => {

  return (
    <Layout.Root content={ children }>
      <Layout.Container>
        <Layout.TopRow/>
        <Layout.NavBar/>
        { children }
      </Layout.Container>
    </Layout.Root>
  );
};

export default LayoutSearchResults;
