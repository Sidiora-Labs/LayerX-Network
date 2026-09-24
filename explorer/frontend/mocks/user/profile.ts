import type { UserInfo } from 'types/api/account';

export const base: UserInfo = {
  avatar: 'https://avatars.githubusercontent.com/u/22130104',
  email: 'tom@ohhhh.me',
  name: 'tom goriunov',
  nickname: 'tom2drum',
  address_hash: null,
};

export const withoutEmail: UserInfo = {
  avatar: 'https://avatars.githubusercontent.com/u/22130104',
  email: null,
  name: 'tom goriunov',
  nickname: 'tom2drum',
  address_hash: '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859',
};

export const withEmailAndWallet: UserInfo = {
  avatar: 'https://avatars.githubusercontent.com/u/22130104',
  email: 'tom@ohhhh.me',
  name: 'tom goriunov',
  nickname: 'tom2drum',
  address_hash: '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859',
};
