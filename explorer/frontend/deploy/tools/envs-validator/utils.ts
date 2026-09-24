import * as yup from 'yup';

export const protocols = [ 'http', 'https' ];

export const urlTest: yup.TestConfig = {
  name: 'url',
  test: (value: unknown) => {
    if (!value) {
      return true;
    }

    try {
      if (typeof value === 'string') {
        new URL(value);
        return true;
      }
    } catch (error) {}

    return false;
  },
  message: '${path} is not a valid URL',
  exclusive: true,
};

export const getYupValidationErrorMessage = (error: unknown) =>
  typeof error === 'object' &&
  error !== null &&
  'errors' in error &&
  Array.isArray(error.errors) ?
    error.errors.join(', ') :
    '';