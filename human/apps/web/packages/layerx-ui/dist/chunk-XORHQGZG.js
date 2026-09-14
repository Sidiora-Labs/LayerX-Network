"use client";

// src/lib/platform.tsx
import * as React from "react";
import { Fragment, jsx } from "react/jsx-runtime";
var PlatformContext = React.createContext("auto");
var ResolvedContext = React.createContext("desktop");
function PlatformProvider({
  value = "auto",
  children
}) {
  return /* @__PURE__ */ jsx(PlatformContext.Provider, { value, children });
}
function useMediaQuery(query) {
  const [matches, setMatches] = React.useState(false);
  React.useEffect(() => {
    const mql = window.matchMedia(query);
    const onChange = () => setMatches(mql.matches);
    onChange();
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, [query]);
  return matches;
}
function usePlatform(override) {
  const fromContext = React.useContext(PlatformContext);
  const setting = override ?? fromContext;
  const isMobileViewport = useMediaQuery("(max-width: 767px)");
  if (setting === "mobile") return "mobile";
  if (setting === "desktop") return "desktop";
  return isMobileViewport ? "mobile" : "desktop";
}
function PlatformSwitch({
  mobile,
  desktop,
  platform
}) {
  const resolved = usePlatform(platform);
  return /* @__PURE__ */ jsx(Fragment, { children: resolved === "mobile" ? mobile : desktop });
}

export {
  PlatformProvider,
  useMediaQuery,
  usePlatform,
  PlatformSwitch
};
//# sourceMappingURL=chunk-XORHQGZG.js.map