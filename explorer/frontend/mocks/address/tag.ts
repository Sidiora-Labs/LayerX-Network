import type { AddressTag, WatchlistName } from 'types/api/addressParams';

export const privateTag: AddressTag = {
  label: 'my-private-tag',
  display_name: 'my private tag',
  address_hash: '0x',
};

export const publicTag: AddressTag = {
  label: 'some-public-tag',
  display_name: 'some public tag',
  address_hash: '0x',
};

export const watchlistName: WatchlistName = {
  label: 'watchlist-name',
  display_name: 'watchlist name',
};
