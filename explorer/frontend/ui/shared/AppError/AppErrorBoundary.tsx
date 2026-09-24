import { chakra } from '@chakra-ui/react';
import React from 'react';

import getErrorCause from 'lib/errors/getErrorCause';
import getErrorCauseStatusCode from 'lib/errors/getErrorCauseStatusCode';
import getErrorMessage from 'lib/errors/getErrorMessage';
import getErrorObjStatusCode from 'lib/errors/getErrorObjStatusCode';
import getErrorStack from 'lib/errors/getErrorStack';
import { useRollbar } from 'lib/rollbar';
import ErrorBoundary from 'ui/shared/ErrorBoundary';

import AppError from './AppError';

interface Props {
  className?: string;
  children: React.ReactNode;
  Container?: React.FC<{ children: React.ReactNode }>;
}

const AppErrorBoundary = ({ className, children, Container }: Props) => {

  const rollbar = useRollbar();

  const renderErrorScreen = React.useCallback((error?: Error) => {
    const content = <AppError error={ error } className={ className }/>;
    if (Container) {
      return <Container>{ content }</Container>;
    }
    return content;
  }, [ className, Container ]);

  const handleError = React.useCallback((error: Error) => {
    const statusCode = getErrorCauseStatusCode(error) || getErrorObjStatusCode(error);
    if (statusCode || !rollbar) {
      // For now, we are not interested in logging errors from the API.
      // If an error from a resource should be logged, please consider passing "logError: true" to the useApiQuery or useApiFetch hook.
      return;
    }

    // To this point, there can only be errors that lead to a page crash.
    // Therefore, we set the error level to "critical."
    rollbar.critical(getErrorMessage(error) ?? 'Application error', {
      cause: getErrorCause(error),
      stack: getErrorStack(error),
    });
  }, [ rollbar ]);

  return (
    <ErrorBoundary renderErrorScreen={ renderErrorScreen } onError={ handleError }>
      { children }
    </ErrorBoundary>
  );
};

export default React.memo(chakra(AppErrorBoundary));
