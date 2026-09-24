import { useRouter } from 'next/router';
import React from 'react';

import useApiQuery from 'lib/api/useApiQuery';
import throwOnResourceLoadError from 'lib/errors/throwOnResourceLoadError';
import getQueryParamString from 'lib/router/getQueryParamString';
import { TAC_OPERATION_DETAILS } from 'stubs/operations';
import TacOperationDetails from 'ui/operation/tac/TacOperationDetails';
import TextAd from 'ui/shared/ad/TextAd';
import OperationEntity from 'ui/shared/entities/operation/OperationEntity';
import PageTitle from 'ui/shared/Page/PageTitle';
import TacOperationTag from 'ui/shared/TacOperationTag';

const TacOperation = () => {
  const router = useRouter();
  const id = getQueryParamString(router.query.id);

  const query = useApiQuery('tac:operation', {
    pathParams: { id },
    queryOptions: {
      placeholderData: TAC_OPERATION_DETAILS,
    },
  });

  throwOnResourceLoadError(query);

  const titleContentAfter = query.data ? (
    <TacOperationTag type={ query.data.type } loading={ query.isPlaceholderData }/>
  ) : null;

  const titleSecondRow = (
    <OperationEntity id={ id } noLink variant="subheading" type={ query.data?.type }/>
  );

  return (
    <>
      <TextAd mb={ 6 }/>
      <PageTitle
        title="Operation details"
        contentAfter={ titleContentAfter }
        isLoading={ query.isPlaceholderData }
        secondRow={ titleSecondRow }
      />
      { query.data && (
        <TacOperationDetails isLoading={ query.isPlaceholderData } data={ query.data }/>
      ) }
    </>
  );
};

export default React.memo(TacOperation);
