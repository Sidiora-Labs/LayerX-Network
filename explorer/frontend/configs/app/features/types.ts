type FeatureEnabled<Payload extends Record<string, unknown> = Record<string, never>> = { title: string; isEnabled: true } & Payload;
type FeatureDisabled = { title: string; isEnabled: false };

export type Feature<Payload extends Record<string, unknown> = Record<string, never>> = FeatureEnabled<Payload> | FeatureDisabled;

// typescript cannot properly resolve unions in nested objects - https://github.com/microsoft/TypeScript/issues/18758
// so we use this little helper where it is needed
export const getFeaturePayload = <Payload extends Record<string, unknown>>(feature: Feature<Payload>): Payload | undefined => {
  return feature.isEnabled ? feature : undefined;
};
