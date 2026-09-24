import React from 'react';
import type { FieldValues, Path } from 'react-hook-form';
import { useFormContext, useWatch } from 'react-hook-form';

import { urlValidator } from '../../validators/url';

interface Params<
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
> {
  name: Name;
  isRequired?: boolean;
}

interface ReturnType {
  input: {
    rules: {
      required?: boolean;
      validate: {
        preview: () => string | true;
      };
    };
    isRequired?: boolean;
    onBlur: () => void;
  };
  preview: {
    src: string | undefined;
    isInvalid: boolean;
    onLoad: () => void;
    onError: () => void;
  };
}

export function useImageField<
  FormFields extends FieldValues,
  Name extends Path<FormFields>,
>({
  name,
  isRequired,
}: Params<FormFields, Name>): ReturnType {
  const { trigger, formState, control } = useFormContext<FormFields>();

  const imageLoadError = React.useRef(false);
  const fieldValue = useWatch({ name, control, exact: true });
  const fieldError = formState.errors[name];

  const [ value, setValue ] = React.useState<string | undefined>(fieldValue);

  const validator = React.useCallback(() => {
    return imageLoadError.current ? 'Unable to load image' : true;
  }, []);

  const onLoad = React.useCallback(() => {
    imageLoadError.current = false;
    trigger(name);
  }, [ name, trigger ]);

  const onError = React.useCallback(() => {
    imageLoadError.current = true;
    trigger(name);
  }, [ name, trigger ]);

  const onBlur = React.useCallback(() => {
    if (!isRequired && !fieldValue) {
      imageLoadError.current = false;
      trigger(name);
      setValue(undefined);
      return;
    }

    const isValidUrl = urlValidator(fieldValue);
    isValidUrl === true && setValue(fieldValue);
  }, [ fieldValue, isRequired, name, trigger ]);

  return React.useMemo(() => {
    return {
      input: {
        isRequired,
        rules: {
          required: isRequired,
          validate: {
            preview: validator,
          },
        },
        onBlur,
      },
      preview: {
        src: fieldError?.type === 'url' ? undefined : value,
        isInvalid: fieldError?.type === 'preview',
        onLoad,
        onError,
      },
    };
  }, [ fieldError?.type, isRequired, onBlur, onError, onLoad, validator, value ]);
}
