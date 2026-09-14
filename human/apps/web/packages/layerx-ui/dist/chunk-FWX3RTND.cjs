"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/primary-action.tsx
var _jsxruntime = require('react/jsx-runtime');
function PrimaryAction({
  children,
  platform,
  position = "footer",
  className,
  ...props
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      "div",
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0,
          "sticky bottom-0 z-20 -mx-4 mt-auto bg-[linear-gradient(to_top,var(--background)_60%,transparent)] px-4 pt-6 pb-[max(1rem,env(safe-area-inset-bottom))]"
        ),
        children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.Button, { size: "lg", fullWidth: true, className, ...props, children })
      }
    );
  }
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    _chunkRA3A4XJ2cjs.Button,
    {
      size: position === "header" ? "md" : "lg",
      className: _chunkMD6ORKN4cjs.cn.call(void 0, position === "footer" && "min-w-[180px]", className),
      ...props,
      children
    }
  );
}



exports.PrimaryAction = PrimaryAction;
//# sourceMappingURL=chunk-FWX3RTND.cjs.map