import type { ApiResource } from '../types';

export type IsPaginated<R extends ApiResource> = R extends { paginated: true } ? true : false;
