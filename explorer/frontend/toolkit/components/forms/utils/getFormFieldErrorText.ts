import type { FieldError } from 'react-hook-form';

export function getFormFieldErrorText(error: FieldError | undefined) {
  if (!error?.message && error?.type === 'pattern') {
    return 'Invalid format';
  }

  return error?.message;
}
