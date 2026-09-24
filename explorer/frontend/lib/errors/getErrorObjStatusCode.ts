import getErrorObj from './getErrorObj';

export default function getErrorObjStatusCode(error: unknown) {
  const errorObj = getErrorObj(error);

  if (!errorObj || !('status' in errorObj) || typeof errorObj.status !== 'number') {
    return;
  }

  return errorObj.status;
}
