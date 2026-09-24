export const EMAIL_REGEXP = /^[\w.%+-]+@[a-z\d-]+(?:\.[a-z\d-]+)+$/i;

export const emailValidator = (value: string) => EMAIL_REGEXP.test(value) ? true : 'Invalid email';
