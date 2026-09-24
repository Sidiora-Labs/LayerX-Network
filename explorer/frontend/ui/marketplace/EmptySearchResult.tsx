import React from 'react';

import { MarketplaceCategory } from 'types/client/marketplace';

import config from 'configs/app';
import { EmptyState } from 'toolkit/chakra/empty-state';
import { Link } from 'toolkit/chakra/link';
import { apos, space } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

const feature = config.features.marketplace;

type Props = {
  favoriteApps: Array<string>;
  selectedCategoryId?: string;
};

const EmptySearchResult = ({ favoriteApps, selectedCategoryId }: Props) => (
  <EmptyState
    description={
      (selectedCategoryId === MarketplaceCategory.FAVORITES && !favoriteApps.length) ? (
        <>
          You don{ apos }t have any favorite apps.<br/>
          Click on the <IconSvg name="heart_outline" boxSize={ 5 } verticalAlign="text-bottom" color="icon.secondary"/>{ space }
          icon on the app{ apos }s card to add it to Favorites.
        </>
      ) : (
        <>
          No matching apps found.
          { 'suggestIdeasFormUrl' in feature && (
            <>
              { ' ' }Have a groundbreaking idea or app suggestion?<br/>
              <Link external href={ feature.suggestIdeasFormUrl }>Share it with us</Link>
            </>
          ) }
        </>
      )
    }
  />
);

export default React.memo(EmptySearchResult);
