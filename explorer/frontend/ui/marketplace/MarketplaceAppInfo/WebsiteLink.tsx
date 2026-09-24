import React from 'react';

import { Link } from 'toolkit/chakra/link';
import { makePrettyLink } from 'toolkit/utils/url';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  url?: string | undefined;
}

const WebsiteLink = ({ url }: Props) => {
  if (!url) {
    return null;
  }

  return (
    <Link
      external
      noIcon
      href={ url }
      display="inline-flex"
      alignItems="center"
      columnGap={ 1 }
      mt={ 3 }
    >
      <IconSvg name="link" boxSize={ 5 } color="icon.primary"/>
      <span>{ makePrettyLink(url)?.domain }</span>
    </Link>
  );
};

export default WebsiteLink;
