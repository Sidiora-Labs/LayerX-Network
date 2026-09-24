import React from 'react';

import type { TimeFormat } from 'types/settings';
import { ADDRESS_FORMATS, type AddressFormat } from 'types/views/address';

import * as cookies from 'lib/cookies';

import { useAppContext } from './app';

interface SettingsProviderProps {
  children: React.ReactNode;
}

interface TSettingsContext {
  addressFormat: AddressFormat;
  toggleAddressFormat: () => void;
  timeFormat: TimeFormat;
  toggleTimeFormat: () => void;
  isLocalTime: boolean;
  toggleIsLocalTime: () => void;
}

export const SettingsContext = React.createContext<TSettingsContext | null>(null);

export function SettingsContextProvider({ children }: SettingsProviderProps) {
  const { cookies: appCookies } = useAppContext();
  const initialAddressFormat = cookies.get(cookies.NAMES.ADDRESS_FORMAT, appCookies);

  const [ addressFormat, setAddressFormat ] = React.useState<AddressFormat>(
    initialAddressFormat && ADDRESS_FORMATS.includes(initialAddressFormat as AddressFormat) ? initialAddressFormat as AddressFormat : 'base16',
  );

  const [ timeFormat, setTimeFormat ] = React.useState<TimeFormat>(
    cookies.get(cookies.NAMES.TIME_FORMAT, appCookies) as TimeFormat || 'relative',
  );

  const [ isLocalTime, setIsLocalTime ] = React.useState<boolean>(
    (cookies.get(cookies.NAMES.LOCAL_TIME, appCookies) ?? 'true') === 'true',
  );

  const toggleAddressFormat = React.useCallback(() => {
    setAddressFormat(prev => {
      const nextValue = prev === 'base16' ? 'bech32' : 'base16';
      cookies.set(cookies.NAMES.ADDRESS_FORMAT, nextValue);
      return nextValue;
    });
  }, []);

  const toggleTimeFormat = React.useCallback(() => {
    setTimeFormat(prev => {
      const nextValue = prev === 'relative' ? 'absolute' : 'relative';
      cookies.set(cookies.NAMES.TIME_FORMAT, nextValue);
      return nextValue;
    });
  }, []);

  const toggleIsLocalTime = React.useCallback(() => {
    setIsLocalTime(prev => {
      const nextValue = !prev;
      cookies.set(cookies.NAMES.LOCAL_TIME, nextValue ? 'true' : 'false');
      return nextValue;
    });
  }, []);

  const value = React.useMemo(() => {
    return {
      addressFormat,
      toggleAddressFormat,
      timeFormat,
      toggleTimeFormat,
      isLocalTime,
      toggleIsLocalTime,
    };
  }, [ addressFormat, toggleAddressFormat, timeFormat, toggleTimeFormat, isLocalTime, toggleIsLocalTime ]);

  return (
    <SettingsContext.Provider value={ value }>
      { children }
    </SettingsContext.Provider>
  );
}

export function useSettingsContext(disabled?: boolean) {
  const context = React.useContext(SettingsContext);
  if (context === undefined || disabled) {
    return null;
  }
  return context;
}
