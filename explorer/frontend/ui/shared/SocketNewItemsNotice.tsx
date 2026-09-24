import { Text, chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';
import { Alert } from 'toolkit/chakra/alert';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';

const flashblocksFeature = config.features.flashblocks;

interface InjectedProps {
  content: React.ReactNode;
}

interface Props {
  type?: 'transaction' | 'token_transfer' | 'deposit' | 'block' | 'flashblock' | 'cross_chain_transaction';
  children?: (props: InjectedProps) => React.JSX.Element;
  className?: string;
  url?: string;
  showErrorAlert?: boolean;
  num?: number;
  isLoading?: boolean;
  onLinkClick?: () => void;
}

const SocketNewItemsNotice = chakra(({ children, className, url, num, showErrorAlert, type = 'transaction', isLoading, onLinkClick }: Props) => {
  const handleLinkClick = React.useCallback(() => {
    onLinkClick ? onLinkClick() : window.location.reload();
  }, [ onLinkClick ]);

  const alertContent = (() => {
    if (showErrorAlert) {
      return 'Live updates temporarily delayed';
    }

    let name;

    switch (type) {
      case 'token_transfer':
        name = 'token transfer';
        break;
      case 'deposit':
        name = 'deposit';
        break;
      case 'block':
        name = 'block';
        break;
      case 'flashblock': {
        if (flashblocksFeature.isEnabled) {
          name = flashblocksFeature.name;
        }
        break;
      }
      case 'cross_chain_transaction':
        name = 'cross chain transaction';
        break;
      default:
        name = 'transaction';
        break;
    }

    if (!num) {
      return `scanning new ${ name }s...`;
    }

    if (type === 'cross_chain_transaction') {
      return (
        <Link href={ url } onClick={ !url ? handleLinkClick : undefined }>More { name }s available</Link>
      );
    }

    return (
      <>
        <Link href={ url } onClick={ !url ? handleLinkClick : undefined }>{ num.toLocaleString() } more { name }{ num > 1 ? 's' : '' }</Link>
        <Text whiteSpace="pre"> ha{ num > 1 ? 've' : 's' } come in</Text>
      </>
    );
  })();

  const content = !isLoading ? (
    <Alert
      className={ className }
      status={ showErrorAlert || !num ? 'warning_table' : 'info' }
      px={ 4 }
      py="6px"
      textStyle="sm"
    >
      { alertContent }
    </Alert>
  ) : <Skeleton className={ className } h="36px" loading/>;

  return children ? children({ content }) : content;
});

export default SocketNewItemsNotice;

export const Desktop = ({ ...props }: Props) => {
  return (
    <SocketNewItemsNotice
      borderRadius={ props.isLoading ? 'sm' : 0 }
      h={ props.isLoading ? 5 : 'auto' }
      maxW={ props.isLoading ? '215px' : undefined }
      w="100%"
      mx={ props.isLoading ? 4 : 0 }
      my={ props.isLoading ? '6px' : 0 }
      { ...props }
    >
      { ({ content }) => <TableRow><TableCell colSpan={ 100 } p={ 0 } _first={{ p: 0 }} _last={{ p: 0 }}>{ content }</TableCell></TableRow> }
    </SocketNewItemsNotice>
  );
};

export const Mobile = ({ ...props }: Props) => {
  return (
    <SocketNewItemsNotice
      borderBottomRadius={ 0 }
      { ...props }
    />
  );
};
