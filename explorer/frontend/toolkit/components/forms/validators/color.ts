export const COLOR_HEX_REGEXP = /^#[a-f\d]{3,6}$/i;

export const colorValidator = (value: unknown) => {
  if (typeof value !== 'string') {
    return true;
  }

  if (!value || value.length === 0) {
    return true;
  }

  if (value.length !== 4 && value.length !== 7) {
    return 'Invalid length';
  }

  if (!COLOR_HEX_REGEXP.test(value)) {
    return 'Invalid hex code';
  }

  return true;
};
