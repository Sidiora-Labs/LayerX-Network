export const noWhitespaceValidator = (value: unknown) => {
  if (typeof value !== 'string') {
    return true;
  }

  if (value === '') {
    return true;
  }

  const trimmedValue = value.replace(/\s/g, '');

  return trimmedValue !== '' || 'Should contain text';
};
