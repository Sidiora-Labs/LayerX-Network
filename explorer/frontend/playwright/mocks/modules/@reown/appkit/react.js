function useAppKit() {
  return {
    open: () => {},
  };
}

function useAppKitState() {
  return {
    isOpen: false,
  };
}

function useAppKitTheme() {
  return {
    setThemeMode: () => {},
  };
}

function createAppKit() {}

export {
  createAppKit,
  useAppKit,
  useAppKitState,
  useAppKitTheme,
};
