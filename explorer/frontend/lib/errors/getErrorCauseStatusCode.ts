import getErrorCause from './getErrorCause';

export default function getErrorCauseStatusCode(error: Error | undefined): number | undefined {
  const cause = getErrorCause(error);
  return cause && 'status' in cause && typeof cause.status === 'number' ? cause.status : undefined;
}
