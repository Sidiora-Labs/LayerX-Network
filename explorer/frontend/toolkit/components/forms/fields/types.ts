import type React from 'react';
import type { ControllerRenderProps, FieldPathValue, FieldValues, Path, RegisterOptions } from 'react-hook-form';

import type { FieldProps } from '../../../chakra/field';
import type { InputProps } from '../../../chakra/input';
import type { InputGroupProps } from '../../../chakra/input-group';
import type { TextareaProps } from '../../../chakra/textarea';

export interface FormFieldPropsBase<
  FormFields extends FieldValues,
  Name extends Path<FormFields> = Path<FormFields>,
> extends Omit<FieldProps, 'children'> {
  name: Name;
  placeholder: string;
  rules?: Omit<RegisterOptions<FormFields, Name>, 'valueAsNumber' | 'valueAsDate' | 'setValueAs' | 'disabled'>;
  onBlur?: () => void;
  onChange?: () => void;
  inputProps?: InputProps | TextareaProps;
  controllerProps?: {
    shouldUnregister?: boolean;
    defaultValue?: FieldPathValue<FormFields, Name>;
  };
  group?: Omit<InputGroupProps, 'children' | 'endElement'> & {
    endElement?: React.ReactNode | (({ field }: { field: ControllerRenderProps<FormFields, Name> }) => React.ReactNode);
  };

}
