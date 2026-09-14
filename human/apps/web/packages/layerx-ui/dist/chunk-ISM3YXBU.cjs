"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/input.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
var Input = React.forwardRef(
  ({ className, error, leading, trailing, ...props }, ref) => {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
      "div",
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0,
          "flex h-12 items-center gap-2 rounded-md border bg-surface px-4 transition-colors",
          error ? "border-destructive focus-within:ring-2 focus-within:ring-destructive/25" : "border-border focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20",
          props.disabled && "opacity-50",
          className
        ),
        children: [
          leading,
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
            "input",
            {
              ref,
              className: "h-full w-full min-w-0 bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground",
              ...props
            }
          ),
          trailing
        ]
      }
    );
  }
);
Input.displayName = "Input";
var SearchInput = React.forwardRef(
  ({ className, value, onClear, ...props }, ref) => {
    const hasValue = value !== void 0 ? String(value).length > 0 : false;
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
      "div",
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0,
          "flex h-11 items-center gap-2.5 rounded-full bg-surface border border-border px-4 transition-colors focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20",
          className
        ),
        children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Search, { className: "size-[18px] shrink-0 text-muted-foreground", "aria-hidden": true }),
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
            "input",
            {
              ref,
              type: "search",
              value,
              className: "h-full w-full min-w-0 bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground [&::-webkit-search-cancel-button]:hidden",
              ...props
            }
          ),
          hasValue && onClear && /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
            "button",
            {
              type: "button",
              onClick: onClear,
              "aria-label": "Clear search",
              className: "shrink-0 text-faint-foreground hover:text-muted-foreground",
              children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.X, { className: "size-4" })
            }
          )
        ]
      }
    );
  }
);
SearchInput.displayName = "SearchInput";




exports.Input = Input; exports.SearchInput = SearchInput;
//# sourceMappingURL=chunk-ISM3YXBU.cjs.map