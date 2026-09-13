"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/switch.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _reactswitch = require('@radix-ui/react-switch'); var SwitchPrimitive = _interopRequireWildcard(_reactswitch);
var _jsxruntime = require('react/jsx-runtime');
var Switch = React.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
  SwitchPrimitive.Root,
  {
    ref,
    className: _chunkMD6ORKN4cjs.cn.call(void 0, 
      "peer inline-flex h-[26px] w-[46px] shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors outline-none",
      "bg-border-strong data-[state=checked]:bg-accent",
      "focus-visible:ring-2 focus-visible:ring-accent/30 disabled:cursor-not-allowed disabled:opacity-50",
      className
    ),
    ...props,
    children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      SwitchPrimitive.Thumb,
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0, 
          "pointer-events-none block size-[22px] rounded-full bg-white shadow-sm ring-0 transition-transform",
          "translate-x-[2px] data-[state=checked]:translate-x-[22px]"
        )
      }
    )
  }
));
Switch.displayName = "Switch";



exports.Switch = Switch;
//# sourceMappingURL=chunk-DPCK6XTT.cjs.map