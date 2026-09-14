"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";

// src/lib/platform.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _jsxruntime = require('react/jsx-runtime');
var PlatformContext = React.createContext("auto");
var ResolvedContext = React.createContext("desktop");
function PlatformProvider({
  value = "auto",
  children
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, PlatformContext.Provider, { value, children });
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
  const setting = _nullishCoalesce(override, () => ( fromContext));
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
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _jsxruntime.Fragment, { children: resolved === "mobile" ? mobile : desktop });
}






exports.PlatformProvider = PlatformProvider; exports.useMediaQuery = useMediaQuery; exports.usePlatform = usePlatform; exports.PlatformSwitch = PlatformSwitch;
//# sourceMappingURL=chunk-I62LU2PG.cjs.map