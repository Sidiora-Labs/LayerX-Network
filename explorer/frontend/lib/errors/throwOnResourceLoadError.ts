import type { ResourceError, ResourceName } from 'lib/api/resources';

type Params = ({
  isError: true;
  error: ResourceError<unknown>;
} | {
  isError: false;
  error: null;
}) & {
  resource?: ResourceName;
};

export const RESOURCE_LOAD_ERROR_MESSAGE = 'Resource load error';

export default function throwOnResourceLoadError({ isError, error, resource }: Params) {
  if (isError) {
    throw Error(RESOURCE_LOAD_ERROR_MESSAGE, { cause: { ...error, resource } as unknown as Error });
  }
}
