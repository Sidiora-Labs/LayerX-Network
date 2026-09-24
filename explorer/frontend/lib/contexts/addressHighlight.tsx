import React from 'react';

interface AddressHighlightProviderProps {
  children: React.ReactNode;
}

interface TAddressHighlightContext {
  onMouseEnter: (event: React.MouseEvent) => void;
  onMouseLeave: (event: React.MouseEvent) => void;
}

export const AddressHighlightContext = React.createContext<TAddressHighlightContext | null>(null);

export function AddressHighlightProvider({ children }: AddressHighlightProviderProps) {
  const timeoutId = React.useRef<number | null>(null);
  const hashRef = React.useRef<string | null>(null);

  const onMouseEnter = React.useCallback((event: React.MouseEvent) => {
    const hash = event.currentTarget.getAttribute('data-hash');
    if (hash) {
      hashRef.current = hash;
      timeoutId.current = window.setTimeout(() => {
        // for better performance we update DOM-nodes directly bypassing React reconciliation
        const nodes = window.document.querySelectorAll(`[data-hash="${ hashRef.current }"]`);
        for (const node of nodes) {
          node.classList.add('address-entity_highlighted');
        }
      }, 100);
    }
  }, []);

  const onMouseLeave = React.useCallback(() => {
    if (hashRef.current) {
      const nodes = window.document.querySelectorAll(`[data-hash="${ hashRef.current }"]`);
      for (const node of nodes) {
        node.classList.remove('address-entity_highlighted');
      }
      hashRef.current = null;
    }
    typeof timeoutId.current === 'number' && window.clearTimeout(timeoutId.current);
  }, []);

  const value = React.useMemo(() => {
    return {
      onMouseEnter,
      onMouseLeave,
    };
  }, [ onMouseEnter, onMouseLeave ]);

  React.useEffect(() => {
    return () => {
      typeof timeoutId.current === 'number' && window.clearTimeout(timeoutId.current);
    };
  }, []);

  return (
    <AddressHighlightContext.Provider value={ value }>
      { children }
    </AddressHighlightContext.Provider>
  );
}

export function useAddressHighlightContext(disabled?: boolean) {
  const context = React.useContext(AddressHighlightContext);
  if (context === undefined || disabled) {
    return null;
  }
  return context;
}
