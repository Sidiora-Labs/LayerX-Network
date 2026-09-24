export default function getErrorObj(error: unknown) {
  if (typeof error !== 'object') {
    return;
  }

  if (Array.isArray(error)) {
    return;
  }

  if (error === null) {
    return;
  }

  return error;
}
