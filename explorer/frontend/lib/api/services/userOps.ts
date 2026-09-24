import type { ApiResource } from '../types';

export const USER_OPS_API_RESOURCES = {
} satisfies Record<string, ApiResource>;

export type UserOpsApiResourceName = `userOps:${ keyof typeof USER_OPS_API_RESOURCES }`;

export type UserOpsApiResourcePayload = never;
