import getErrorObj from './getErrorObj';

export default function getErrorObjPayload<Payload extends object>(error: unknown): Payload | undefined {
  const errorObj = getErrorObj(error);

  if (!errorObj || !('payload' in errorObj)) {
    return;
  }

  if (typeof errorObj.payload !== 'object') {
    return;
  }

  if (errorObj === null) {
    return;
  }

  if (Array.isArray(errorObj)) {
    return;
  }

  return errorObj.payload as Payload;
}
