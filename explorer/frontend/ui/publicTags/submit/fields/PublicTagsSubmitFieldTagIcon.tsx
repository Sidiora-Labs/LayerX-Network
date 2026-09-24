import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { FormFields } from '../types';

import { FormFieldUrl } from 'toolkit/components/forms/fields/FormFieldUrl';
import { FormFieldImagePreview } from 'toolkit/components/forms/fields/image/FormFieldImagePreview';
import { useImageField } from 'toolkit/components/forms/fields/image/useImageField';
import IconSvg from 'ui/shared/IconSvg';

import PublicTagsSubmitFieldTagIconPreview from './PublicTagsSubmitFieldTagIconPreview';

interface Props {
  index: number;
}

const PublicTagsSubmitFieldTagIcon = ({ index }: Props) => {

  const imageField = useImageField({ name: `tags.${ index }.iconUrl`, isRequired: false });

  return (
    <Flex columnGap={ 3 }>
      <FormFieldUrl<FormFields>
        name={ `tags.${ index }.iconUrl` }
        placeholder="Label icon URL"
        { ...imageField.input }
      />
      <PublicTagsSubmitFieldTagIconPreview url={ imageField.preview.src } isInvalid={ imageField.preview.isInvalid }>
        <FormFieldImagePreview
          { ...imageField.preview }
          fallback={ <IconSvg name="blobs/image" color="icon.primary" boxSize="30px"/> }
          boxSize="30px"
        />
      </PublicTagsSubmitFieldTagIconPreview>
    </Flex>
  );
};

export default React.memo(PublicTagsSubmitFieldTagIcon);
