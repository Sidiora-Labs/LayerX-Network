import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { Fields } from '../types';

import type { FieldProps } from 'toolkit/chakra/field';
import { FormFieldUrl } from 'toolkit/components/forms/fields/FormFieldUrl';
import { FormFieldImagePreview } from 'toolkit/components/forms/fields/image/FormFieldImagePreview';
import { useImageField } from 'toolkit/components/forms/fields/image/useImageField';
import { times } from 'toolkit/utils/htmlEntities';
import TokenLogoPlaceholder from 'ui/shared/TokenLogoPlaceholder';

import TokenInfoIconPreview from '../TokenInfoIconPreview';

interface Props {
  readOnly?: boolean;
  size?: FieldProps['size'];
}

const TokenInfoFieldIconUrl = ({ readOnly, size }: Props) => {

  const imageField = useImageField({ name: 'icon_url', isRequired: true });

  return (
    <Flex columnGap={ 5 }>
      <FormFieldUrl<Fields>
        name="icon_url"
        placeholder={ `Link to icon URL, link to download a SVG or 48${ times }48 PNG icon logo` }
        readOnly={ readOnly }
        size={ size }
        { ...imageField.input }
      />
      <TokenInfoIconPreview url={ imageField.preview.src } isInvalid={ imageField.preview.isInvalid }>
        <FormFieldImagePreview
          { ...imageField.preview }
          fallback={ <TokenLogoPlaceholder boxSize={ 10 }/> }
          boxSize={ 10 }
          borderRadius="base"
        />
      </TokenInfoIconPreview>
    </Flex>
  );
};

export default React.memo(TokenInfoFieldIconUrl);
