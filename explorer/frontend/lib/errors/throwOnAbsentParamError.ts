export const ABSENT_PARAM_ERROR_MESSAGE = 'Required param not provided';

export default function throwOnAbsentParamError(param: unknown) {
  if (!param) {
    throw new Error(ABSENT_PARAM_ERROR_MESSAGE, { cause: { status: 404 } });
  }
}
