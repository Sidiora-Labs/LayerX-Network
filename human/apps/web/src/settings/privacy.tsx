"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { copyEntry } from "../../copy/runtime";

const STORAGE_PREFIX = "layerx.privacy-mode.v1";

interface PrivacyModeContextValue {
  readonly masked: boolean;
  readonly setMasked: (masked: boolean) => void;
}

const PrivacyModeContext = createContext<PrivacyModeContextValue | undefined>(undefined);

function storageKey(principalScope: string): string {
  return `${STORAGE_PREFIX}.${principalScope}`;
}

export function PrivacyModeProvider({
  principalScope,
  children,
}: Readonly<{ principalScope: string; children: ReactNode }>) {
  const key = useMemo(() => storageKey(principalScope), [principalScope]);
  const [masked, setMaskedState] = useState(false);

  useEffect(() => {
    try {
      setMaskedState(window.localStorage.getItem(key) === "masked");
    } catch {
      setMaskedState(false);
    }
    const sync = (event: StorageEvent) => {
      if (event.key === key) {
        setMaskedState(event.newValue === "masked");
      }
    };
    window.addEventListener("storage", sync);
    return () => { window.removeEventListener("storage", sync); };
  }, [key]);

  const setMasked = useCallback((next: boolean) => {
    setMaskedState(next);
    try {
      window.localStorage.setItem(key, next ? "masked" : "visible");
    } catch {
      setMaskedState(next);
    }
  }, [key]);

  const value = useMemo(() => ({ masked, setMasked }), [masked, setMasked]);
  return (
    <PrivacyModeContext.Provider value={value}>
      <div data-privacy-mode={masked ? "masked" : "visible"}>{children}</div>
    </PrivacyModeContext.Provider>
  );
}

export function usePrivacyMode(): PrivacyModeContextValue {
  const value = useContext(PrivacyModeContext);
  if (value === undefined) {
    throw new Error("PrivacyModeProvider is required on authenticated surfaces");
  }
  return value;
}

export function PrivateFigure({
  children,
  className,
}: Readonly<{ children: ReactNode; className?: string }>) {
  const { masked } = usePrivacyMode();
  if (masked) {
    return (
      <span
        className={className}
        data-private-figure="masked"
        aria-label={copyEntry("privacy.hidden").message}
      >
        ••••
      </span>
    );
  }
  return <span className={className} data-private-figure="visible">{children}</span>;
}
