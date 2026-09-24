import React from 'react';

import useIsMounted from 'lib/hooks/useIsMounted';

interface Props {
  children: React.ReactNode;
  content: React.ReactNode;
}

const Root = ({ children, content }: Props) => {
  const isMounted = useIsMounted();

  if (!isMounted) {
    return content;
  }

  return children;
};

export default React.memo(Root);
