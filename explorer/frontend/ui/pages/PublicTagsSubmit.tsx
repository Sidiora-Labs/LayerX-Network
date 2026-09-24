import React from 'react';

import type { FormSubmitResult } from 'ui/publicTags/submit/types';

import useApiQuery from 'lib/api/useApiQuery';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import PublicTagsSubmitForm from 'ui/publicTags/submit/PublicTagsSubmitForm';
import PublicTagsSubmitResult from 'ui/publicTags/submit/PublicTagsSubmitResult';
import DataFetchAlert from 'ui/shared/DataFetchAlert';
import PageTitle from 'ui/shared/Page/PageTitle';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';

type Screen = 'form' | 'result' | 'initializing' | 'error';

const PublicTagsSubmit = () => {

  const [ screen, setScreen ] = React.useState<Screen>('initializing');
  const [ submitResult, setSubmitResult ] = React.useState<FormSubmitResult>();

  const profileQuery = useProfileQuery();
  const configQuery = useApiQuery('metadata:public_tag_types', { queryOptions: { enabled: !profileQuery.isLoading } });

  React.useEffect(() => {
    if (!configQuery.isPending) {
      setScreen(configQuery.isError ? 'error' : 'form');
    }
  }, [ configQuery.isError, configQuery.isPending ]);

  const handleFormSubmitResult = React.useCallback((result: FormSubmitResult) => {
    setSubmitResult(result);
    setScreen('result');
  }, []);

  const content = (() => {
    switch (screen) {
      case 'initializing':
        return <ContentLoader/>;
      case 'error':
        return <DataFetchAlert/>;
      case 'form':
        return <PublicTagsSubmitForm config={ configQuery.data } onSubmitResult={ handleFormSubmitResult } userInfo={ profileQuery.data }/>;
      case 'result':
        return <PublicTagsSubmitResult data={ submitResult }/>;
      default:
        return null;
    }
  })();

  return (
    <>
      <PageTitle title="Request a public tag/label"/>
      { content }
    </>
  );
};

export default PublicTagsSubmit;
