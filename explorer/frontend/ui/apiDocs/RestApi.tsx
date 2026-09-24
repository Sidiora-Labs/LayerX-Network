import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import useApiQuery from 'lib/api/useApiQuery';
import { AccordionItem, AccordionItemContent, AccordionItemTrigger, AccordionRoot, useAccordion } from 'toolkit/chakra/accordion';
import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';
import CopyToClipboard from 'ui/shared/CopyToClipboard';

import SwaggerUI from './SwaggerUI';
import { REST_API_SECTIONS } from './utils';

const RestApi = () => {

  const configQuery = useApiQuery('general:config_backend', {
    queryOptions: {
      refetchOnMount: false,
      enabled: config.features.apiDocs.isEnabled,
    },
  });
  const configVersionQuery = useApiQuery('general:config_backend_version', {
    queryOptions: {
      refetchOnMount: false,
      enabled: config.features.apiDocs.isEnabled,
    },
  });

  const isLoading = config.features.apiDocs.isEnabled ? configQuery.isPending || configVersionQuery.isPending : false;

  const { value, onValueChange, scrollToItemFromUrl } = useAccordion(REST_API_SECTIONS);

  React.useEffect(() => {
    if (!isLoading) {
      scrollToItemFromUrl();
    }
    // runs only on mount once all queries are resolved
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ isLoading ]);

  const coreApiSwaggerUrl = React.useMemo(() => {
    if (isLoading || !config.features.apiDocs.isEnabled) {
      return;
    }

    // v[release_number]+commit.[commit_hash]
    // v10.0.0-alpha.17 or v9.3.2+commit.a7ab3460
    const version = configVersionQuery.data?.backend_version;
    const releaseNumber = version?.match(/^v(\d+\.\d+\.\d+)/)?.[1];
    const folderName = configQuery.data?.openapi_spec_folder_name;

    if (releaseNumber && folderName) {
      return `https://raw.githubusercontent.com/blockscout/swaggers/master/blockscout/${ releaseNumber }/${ folderName }/swagger.yaml`;
    }
  }, [ configQuery.data?.openapi_spec_folder_name, configVersionQuery.data?.backend_version, isLoading ]);

  const sections = React.useMemo(() => {
    if (!coreApiSwaggerUrl) {
      return REST_API_SECTIONS;
    }

    return REST_API_SECTIONS.map((section) => {
      if (section.id === 'blockscout-core-api') {
        return {
          ...section,
          swagger: {
            ...section.swagger,
            url: coreApiSwaggerUrl,
          },
        };
      }
      return section;
    });
  }, [ coreApiSwaggerUrl ]);

  if (isLoading) {
    return <ContentLoader/>;
  }

  if (sections.length === 0) {
    return null;
  }

  if (sections.length === 1) {
    return <SwaggerUI { ...sections[0].swagger }/>;
  }

  return (
    <AccordionRoot onValueChange={ onValueChange } value={ value }>
      { sections.map((section, index) => (
        <AccordionItem key={ index } value={ section.id }>
          <AccordionItemTrigger>
            <CopyToClipboard
              text={ config.app.baseUrl + route({ pathname: '/api-docs', query: { tab: 'rest_api' }, hash: section.id }) }
              type="link"
              ml={ 0 }
              mr={ 1 }
              as="div"
            />
            { section.title }
          </AccordionItemTrigger>
          <AccordionItemContent>
            <SwaggerUI { ...section.swagger }/>
          </AccordionItemContent>
        </AccordionItem>
      )) }
    </AccordionRoot>
  );
};

export default React.memo(RestApi);
