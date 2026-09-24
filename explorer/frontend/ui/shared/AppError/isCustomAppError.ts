import type { ResourceError } from 'lib/api/resources';

// status codes when custom error screen should be shown
const CUSTOM_STATUS_CODES = [ 403, 404, 422, 429 ];

export default function isCustomAppError(error: ResourceError<unknown>) {
  return CUSTOM_STATUS_CODES.includes(error.status);
}
