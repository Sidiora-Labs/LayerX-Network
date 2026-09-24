import { Box } from '@chakra-ui/react';
import React from 'react';

import type { SmartContractSecurityAuditSubmission } from 'types/api/contract';

import useApiQuery from 'lib/api/useApiQuery';
import dayjs from 'lib/date/dayjs';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import ContainerWithScrollY from 'ui/shared/ContainerWithScrollY';
import FormModal from 'ui/shared/FormModal';

import ContractSubmitAuditForm from './ContractSubmitAuditForm';

type Props = {
  addressHash?: string;
};

const ContractSecurityAudits = ({ addressHash }: Props) => {
  const { data, isPlaceholderData } = useApiQuery('general:contract_security_audits', {
    pathParams: { hash: addressHash },
    queryOptions: {
      refetchOnMount: false,
      placeholderData: { items: [] },
      enabled: Boolean(addressHash),
    },
  });

  const formTitle = 'Submit audit';

  const modalProps = useDisclosure();

  const renderForm = React.useCallback(() => {
    return <ContractSubmitAuditForm address={ addressHash } onSuccess={ modalProps.onClose }/>;
  }, [ addressHash, modalProps.onClose ]);

  return (
    <>
      { data?.items && data.items.length > 0 && (
        <Box position="relative">
          <ContainerWithScrollY
            gradientHeight={ 24 }
            rowGap={ 1 }
            w="100%"
            maxH="80px"
            mb={ 2 }
          >
            { data.items.map(item => (
              <Link external href={ item.audit_report_url } key={ item.audit_company_name + item.audit_publish_date } loading={ isPlaceholderData }>
                { `${ item.audit_company_name }, ${ dayjs(item.audit_publish_date).format('MMM DD, YYYY') }` }
              </Link>
            )) }
          </ContainerWithScrollY>
        </Box>
      ) }
      <Button variant="outline" size="sm" onClick={ modalProps.onOpen }>Submit audit</Button>
      <FormModal<SmartContractSecurityAuditSubmission>
        open={ modalProps.open }
        onOpenChange={ modalProps.onOpenChange }
        title={ formTitle }
        renderForm={ renderForm }
      />
    </>
  );
};

export default React.memo(ContractSecurityAudits);
