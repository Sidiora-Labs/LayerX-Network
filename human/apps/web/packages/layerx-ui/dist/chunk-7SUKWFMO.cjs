"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/popover.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _reactpopover = require('@radix-ui/react-popover'); var PopoverPrimitive = _interopRequireWildcard(_reactpopover);
var _jsxruntime = require('react/jsx-runtime');
var Popover = PopoverPrimitive.Root;
var PopoverTrigger = PopoverPrimitive.Trigger;
var PopoverContent = React.forwardRef(({ className, align = "start", sideOffset = 8, ...props }, ref) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, PopoverPrimitive.Portal, { children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
  PopoverPrimitive.Content,
  {
    ref,
    align,
    sideOffset,
    className: _chunkMD6ORKN4cjs.cn.call(void 0, 
      "z-50 w-auto min-w-[220px] rounded-lg border border-border bg-surface p-2 shadow-overlay outline-none",
      "data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out",
      className
    ),
    ...props
  }
) }));
PopoverContent.displayName = "PopoverContent";





exports.Popover = Popover; exports.PopoverTrigger = PopoverTrigger; exports.PopoverContent = PopoverContent;
//# sourceMappingURL=chunk-7SUKWFMO.cjs.map