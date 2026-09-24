import { List, chakra, Box } from '@chakra-ui/react';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'next/router';
import React from 'react';

import type { VerifiedAddress, TokenInfoApplication, TokenInfoApplications, VerifiedAddressResponse } from 'types/api/account';

import config from 'configs/app';
import useApiQuery, { getResourceKey } from 'lib/api/useApiQuery';
import { PAGE_TYPE_DICT } from 'lib/mixpanel/getPageType';
import getQueryParamString from 'lib/router/getQueryParamString';
import { TOKEN_INFO_APPLICATION, VERIFIED_ADDRESS } from 'stubs/account';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { BackToButton } from 'toolkit/components/buttons/BackToButton';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import AddressVerificationModal from 'ui/addressVerification/AddressVerificationModal';
import AccountPageDescription from 'ui/shared/AccountPageDescription';
import DataListDisplay from 'ui/shared/DataListDisplay';
import PageTitle from 'ui/shared/Page/PageTitle';
import AdminSupportText from 'ui/shared/texts/AdminSupportText';
import useProfileQuery from 'ui/snippets/auth/useProfileQuery';
import useRedirectForInvalidAuthToken from 'ui/snippets/auth/useRedirectForInvalidAuthToken';
import TokenInfoForm from 'ui/tokenInfo/TokenInfoForm';
import VerifiedAddressesEmailAlert from 'ui/verifiedAddresses/VerifiedAddressesEmailAlert';
import VerifiedAddressesListItem from 'ui/verifiedAddresses/VerifiedAddressesListItem';
import VerifiedAddressesTable from 'ui/verifiedAddresses/VerifiedAddressesTable';

const VerifiedAddresses = () => {
  useRedirectForInvalidAuthToken();

  const router = useRouter();
  const addressHash = getQueryParamString(router.query.address);

  const [ selectedAddress, setSelectedAddress ] = React.useState<string | undefined>(addressHash);

  React.useEffect(() => {
    addressHash && router.replace({ pathname: '/account/verified-addresses' });
  // componentDidMount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ ]);

  const modalProps = useDisclosure();
  const queryClient = useQueryClient();

  const profileQuery = useProfileQuery();

  const addressesQuery = useApiQuery('contractInfo:verified_addresses', {
    pathParams: { chainId: config.chain.id },
    queryOptions: {
      placeholderData: { verifiedAddresses: Array(3).fill(VERIFIED_ADDRESS) },
      enabled: Boolean(profileQuery.data?.email),
    },
  });
  const applicationsQuery = useApiQuery('admin:token_info_applications', {
    pathParams: { chainId: config.chain.id, id: undefined },
    queryOptions: {
      placeholderData: { submissions: Array(3).fill(TOKEN_INFO_APPLICATION) },
      enabled: Boolean(profileQuery.data?.email),
      select: (data) => {
        return {
          ...data,
          submissions: data.submissions.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)),
        };
      },
    },
  });

  const isLoading = addressesQuery.isPlaceholderData || applicationsQuery.isPlaceholderData;
  const userWithoutEmail = profileQuery.data && !profileQuery.data.email;

  const handleGoBack = React.useCallback(() => {
    setSelectedAddress(undefined);
  }, []);

  const handleItemAdd = React.useCallback((address: string) => {
    setSelectedAddress(address);
  }, []);
  const handleItemEdit = React.useCallback((address: string) => {
    setSelectedAddress(address);
  }, []);

  const handleAddressSubmit = React.useCallback((newItem: VerifiedAddress) => {
    queryClient.setQueryData(
      getResourceKey('contractInfo:verified_addresses', { pathParams: { chainId: config.chain.id } }),
      (prevData: VerifiedAddressResponse | undefined) => {
        if (!prevData) {
          return { verifiedAddresses: [ newItem ] };
        }

        return {
          verifiedAddresses: [ newItem, ...prevData.verifiedAddresses ],
        };
      });
  }, [ queryClient ]);

  const handleApplicationSubmit = React.useCallback((newItem: TokenInfoApplication) => {
    setSelectedAddress(undefined);
    queryClient.setQueryData(
      getResourceKey('admin:token_info_applications', { pathParams: { chainId: config.chain.id, id: undefined } }),
      (prevData: TokenInfoApplications | undefined) => {
        if (!prevData) {
          return { submissions: [ newItem ] };
        }

        const isExisting = prevData.submissions.some((item) => item.id.toLowerCase() === newItem.id.toLowerCase());
        const submissions = isExisting ?
          prevData.submissions.map((item) => item.id.toLowerCase() === newItem.id.toLowerCase() ? newItem : item) :
          [ newItem, ...prevData.submissions ];
        return { submissions };
      });
  }, [ queryClient ]);

  const addButton = (() => {
    if (userWithoutEmail) {
      return (
        <Button disabled mt={ 8 }>
          Add address
        </Button>
      );
    }

    return (
      <Button onClick={ modalProps.onOpen } loadingSkeleton={ isLoading } mt={ 8 }>
        Add address
      </Button>
    );
  })();

  if (selectedAddress) {
    const addressInfo = addressesQuery.data?.verifiedAddresses.find(({ contractAddress }) => contractAddress.toLowerCase() === selectedAddress.toLowerCase());
    const tokenName = addressInfo ? `${ addressInfo.metadata.tokenName } (${ addressInfo.metadata.tokenSymbol })` : '';
    const beforeTitle = <BackToButton onClick={ handleGoBack } hint="Back to my verified addresses" mr={ 3 }/>;

    return (
      <>
        <PageTitle title="Token info application form" beforeTitle={ beforeTitle }/>
        <TokenInfoForm
          address={ selectedAddress }
          tokenName={ tokenName }
          application={ applicationsQuery.data?.submissions.find(({ tokenAddress }) => tokenAddress.toLowerCase() === selectedAddress.toLowerCase()) }
          onSubmit={ handleApplicationSubmit }
        />
      </>
    );
  }

  const content = (() => {
    if (userWithoutEmail) {
      return null;
    }

    if (addressesQuery.data?.verifiedAddresses) {
      return (
        <>
          <Box hideFrom="lg" key="content-mobile">
            { addressesQuery.data.verifiedAddresses.map((item, index) => (
              <VerifiedAddressesListItem
                key={ item.contractAddress + (isLoading ? index : '') }
                item={ item }
                application={
                  applicationsQuery.data?.submissions
                    ?.find(({ tokenAddress }) => tokenAddress.toLowerCase() === item.contractAddress.toLowerCase())
                }
                onAdd={ handleItemAdd }
                onEdit={ handleItemEdit }
                isLoading={ isLoading }
              />
            )) }
          </Box>
          <Box hideBelow="lg" key="content-desktop">
            <VerifiedAddressesTable
              data={ addressesQuery.data.verifiedAddresses }
              applications={ applicationsQuery.data?.submissions }
              onItemEdit={ handleItemEdit }
              onItemAdd={ handleItemAdd }
              isLoading={ isLoading }
            />
          </Box>
        </>
      );
    }

    return null;
  })();

  return (
    <>
      <PageTitle title="My verified addresses"/>
      { userWithoutEmail && <VerifiedAddressesEmailAlert/> }
      <AccountPageDescription allowCut={ false }>
        <span>
          Verify ownership of a smart contract address to easily update information in Blockscout.
          You will sign a single message to verify contract ownership.
          Once verified, you can update token information, address name tags, and address labels from the
          Blockscout console without needing to sign additional messages.
        </span>
        <chakra.p fontWeight={ 600 } mt={ 5 }>
          Before starting, make sure that:
        </chakra.p>
        <List.Root pl={ 5 } as="ol">
          <List.Item>The source code for the smart contract is deployed on “{ config.chain.name }”.</List.Item>
          <List.Item>
            <span>The source code is verified (if not yet verified, you can use </span>
            <Link href="https://docs.blockscout.com/devs/verification" external noIcon>this tool</Link>
            <span>).</span>
          </List.Item>
        </List.Root>
        <chakra.div mt={ 5 }>
          Once these steps are complete, click the Add address button below to get started.
        </chakra.div>
        <AdminSupportText mt={ 5 }/>
      </AccountPageDescription>
      <DataListDisplay
        isError={ profileQuery.isError || addressesQuery.isError || applicationsQuery.isError }
        itemsNum={ addressesQuery.data?.verifiedAddresses.length }
        emptyText=""
      >
        { content }
      </DataListDisplay>
      { addButton }
      <AddressVerificationModal
        pageType={ PAGE_TYPE_DICT['/account/verified-addresses'] }
        open={ modalProps.open }
        onOpenChange={ modalProps.onOpenChange }
        onSubmit={ handleAddressSubmit }
        onAddTokenInfoClick={ handleItemAdd }
        onShowListClick={ modalProps.onClose }
      />
    </>
  );
};

export default React.memo(VerifiedAddresses);
