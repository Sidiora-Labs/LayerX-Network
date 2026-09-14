"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/option-list.tsx
var _reactradiogroup = require('@radix-ui/react-radio-group'); var RadioGroup = _interopRequireWildcard(_reactradiogroup);
var _jsxruntime = require('react/jsx-runtime');
function OptionList({
  items,
  value,
  onValueChange,
  className,
  "aria-label": ariaLabel
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    RadioGroup.Root,
    {
      value,
      onValueChange,
      className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col divide-y divide-border/70", className),
      "aria-label": ariaLabel,
      children: items.map((item) => {
        const checked = item.value === value;
        return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
          RadioGroup.Item,
          {
            value: item.value,
            className: "group flex w-full cursor-pointer items-center justify-between gap-3 py-4 text-left outline-none",
            children: [
              /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex min-w-0 flex-col gap-0.5", children: [
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[15px] font-medium text-foreground", children: item.label }),
                item.description && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[13px] text-muted-foreground", children: item.description })
              ] }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
                "span",
                {
                  className: _chunkMD6ORKN4cjs.cn.call(void 0,
                    "inline-flex size-[22px] shrink-0 items-center justify-center rounded-full border-2 transition-colors",
                    checked ? "border-accent" : "border-border-strong group-hover:border-faint-foreground"
                  ),
                  "aria-hidden": true,
                  children: checked && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "size-3 rounded-full bg-accent" })
                }
              )
            ]
          },
          item.value
        );
      })
    }
  );
}



exports.OptionList = OptionList;
//# sourceMappingURL=chunk-IOJALOHM.cjs.map