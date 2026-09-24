import getErrorObj from './getErrorObj';

export default function getErrorProp<T extends unknown>(error: unknown, prop: string): T | undefined {
  const errorObj = getErrorObj(error);
  return errorObj && prop in errorObj ?
    (errorObj[prop as keyof typeof errorObj] as T) :
    undefined;
}
