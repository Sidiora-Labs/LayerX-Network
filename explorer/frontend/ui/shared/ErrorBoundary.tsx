import type { NextRouter } from 'next/router';
import { useRouter } from 'next/router';
import React from 'react';

interface Props {
  children: React.ReactNode;
  renderErrorScreen: (error?: Error) => React.ReactNode;
  onError?: (error: Error) => void;
}

interface PropsWithRouter extends Props {
  router: NextRouter;
}

interface State {
  hasError: boolean;
  error?: Error;
  errorPathname?: string;
}

class ErrorBoundary extends React.PureComponent<PropsWithRouter, State> {
  state: State = {
    hasError: false,
  };

  static getDerivedStateFromProps(props: PropsWithRouter, state: State) {
    if (state.hasError && state.errorPathname) {
      if (props.router.pathname !== state.errorPathname) {
        return { hasError: false, error: undefined, errorPathname: undefined };
      }
    }

    return null;
  }

  componentDidCatch(error: Error) {
    this.setState({ hasError: true, error, errorPathname: this.props.router.pathname });
    this.props.onError?.(error);
  }

  render() {
    if (this.state.hasError) {
      return this.props.renderErrorScreen(this.state.error);
    }

    return this.props.children;
  }
}

const WrappedErrorBoundary = (props: Props) => {
  const router = useRouter();
  return <ErrorBoundary { ...props } router={ router }/>;
};

export default React.memo(WrappedErrorBoundary);
