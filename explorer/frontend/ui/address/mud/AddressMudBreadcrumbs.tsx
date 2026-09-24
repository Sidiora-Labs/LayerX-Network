import { Box, chakra, Grid } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import useIsMobile from 'lib/hooks/useIsMobile';
import { Link } from 'toolkit/chakra/link';
import { isBrowser } from 'toolkit/utils/isBrowser';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import IconSvg from 'ui/shared/IconSvg';

import useAddressQuery from '../utils/useAddressQuery';

type TableViewProps = {
  className?: string;
  hash: string;
  tableId: string;
  tableName: string;
  recordId?: never;
  recordName?: never;
};

type RecordViewProps = Omit<TableViewProps, 'recordId' | 'recordName'> & {
  recordId: string;
  recordName: string;
};

type BreadcrumbItemProps = {
  text: string;
  href: string;
  isLast?: boolean;
};

const BreadcrumbItem = ({ text, href, isLast }: BreadcrumbItemProps) => {
  const currentUrl = isBrowser() ? window.location.href : '';

  const onLinkClick = React.useCallback(() => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, []);

  if (isLast) {
    return (
      <Grid gap={ 2 } overflow="hidden" templateColumns="auto 24px" alignItems="center">
        <Box
          overflow="hidden"
          whiteSpace="nowrap"
          textOverflow="ellipsis"
        >
          { text }
        </Box>
        <CopyToClipboard text={ currentUrl } type="link" mx={ 0 } color="icon.secondary"/>
      </Grid>
    );
  }

  return (
    <Grid gap={ 2 } overflow="hidden" templateColumns="auto 24px" alignItems="center">
      <Link
        href={ href }
        onClick={ onLinkClick }
        overflow="hidden"
        textOverflow="ellipsis"
        whiteSpace="nowrap"
      >
        { text }
      </Link>
      { !isLast && <IconSvg name="arrows/east" boxSize={ 6 } color={{ _light: 'gray.300', _dark: 'gray.600' }}/> }
    </Grid>
  );
};

const AddressMudBreadcrumbs = (props: TableViewProps | RecordViewProps) => {
  const queryParams = { tab: 'mud', hash: props.hash };
  const isMobile = useIsMobile();

  const addressQuery = useAddressQuery({ hash: props.hash });

  return (
    <Box
      display={ isMobile ? 'flex' : 'grid' }
      flexWrap="wrap"
      gridTemplateColumns="20px auto auto auto"
      gap={ 2 }
      alignItems="center"
      className={ props.className }
      width="fit-content"
      fontSize="sm"
    >
      <IconSvg name="MUD" boxSize={ 5 } color={ addressQuery.data?.is_verified ? 'green.500' : 'icon.primary' }/>
      <BreadcrumbItem
        text="MUD World"
        href={ route({ pathname: '/address/[hash]', query: queryParams }) }
      />
      <BreadcrumbItem
        text={ props.tableName }
        href={ route({ pathname: '/address/[hash]', query: { ...queryParams, table_id: props.tableId } }) }
        isLast={ !('recordId' in props) }
      />
      { ('recordId' in props && typeof props.recordId === 'string') && ('recordName' in props && typeof props.recordName === 'string') && (
        <BreadcrumbItem
          text={ props.recordName }
          href={ route({ pathname: '/address/[hash]', query: { ...queryParams, table_id: props.tableId, record_id: props.recordId } }) }
          isLast

        />
      ) }
    </Box>
  );
};

export default React.memo(chakra(AddressMudBreadcrumbs));
