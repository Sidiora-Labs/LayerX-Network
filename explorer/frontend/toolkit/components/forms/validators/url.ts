export function urlValidator(value: string | undefined) {
  if (!value) {
    return true;
  }

  try {
    new URL(value);
    return true;
  } catch (error) {
    return 'Incorrect URL';
  }
}

export const DOMAIN_REGEXP =
  /(?:[a-z\d](?:[a-z\d-]{0,61}[a-z\d])?\.)+[a-z\d][a-z\d-]{0,61}[a-z\d]/gi;

export function domainValidator(value: string | undefined) {
  if (!value) {
    return true;
  }

  const domain = (() => {
    try {
      const url = new URL(`https://${ value }`);
      return url.hostname;
    } catch (error) {
      return;
    }
  })();

  return domain === value.toLowerCase() || 'Incorrect domain';
}
