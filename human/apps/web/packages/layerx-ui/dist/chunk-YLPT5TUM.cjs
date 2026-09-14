"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";




var _chunkKJXR3TMYcjs = require('./chunk-KJXR3TMY.cjs');




var _chunk2CZ6QC47cjs = require('./chunk-2CZ6QC47.cjs');


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/detail.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function DetailDisclosure({
  open,
  onOpenChange,
  title,
  children,
  mobileVariant = "sheet",
  desktopVariant = "drawer",
  platform,
  portalContainer,
  summary
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  const disclosureId = React.useId();
  if (resolved === "desktop" && desktopVariant === "inline") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "overflow-hidden rounded-lg border border-border bg-surface", children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
        "button",
        {
          type: "button",
          "aria-expanded": open,
          "aria-controls": disclosureId,
          onClick: () => onOpenChange(!open),
          className: "flex w-full items-center justify-between gap-3 px-5 py-4 text-left font-semibold text-foreground transition-colors hover:bg-surface-sunken/40",
          children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { children: _nullishCoalesce(summary, () => ( title)) }),
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
              _lucidereact.ChevronDown,
              {
                className: _chunkMD6ORKN4cjs.cn.call(void 0, "size-4 text-muted-foreground transition-transform", open && "rotate-180")
              }
            )
          ]
        }
      ),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
        "div",
        {
          id: disclosureId,
          role: "region",
          className: _chunkMD6ORKN4cjs.cn.call(void 0,
            "grid transition-[grid-template-rows] duration-300",
            open ? "grid-rows-[1fr]" : "grid-rows-[0fr]"
          ),
          children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "overflow-hidden", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "border-t border-border px-5 py-4", children }) })
        }
      )
    ] });
  }
  if (resolved === "mobile" && mobileVariant === "pushed") {
    if (!open) return null;
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "fixed inset-0 z-50 flex flex-col bg-background pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)] animate-fade-in", children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "header", { className: "flex items-center gap-3 border-b border-border bg-surface px-4 py-3", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.IconButton, { variant: "outline", size: "sm", onClick: () => onOpenChange(false), "aria-label": "Back", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowLeft, {}) }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h2", { className: "text-[17px] font-bold text-foreground", children: title })
      ] }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "lx-scroll flex-1 overflow-y-auto p-4", children })
    ] });
  }
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.Sheet, { open, onOpenChange, portalContainer, children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetHeader, { title }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetBody, { children })
    ] });
  }
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunk2CZ6QC47cjs.Drawer, { open, onOpenChange, portalContainer, children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk2CZ6QC47cjs.DrawerHeader, { title, onClose: () => onOpenChange(false) }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunk2CZ6QC47cjs.DrawerBody, { children })
  ] });
}



exports.DetailDisclosure = DetailDisclosure;
//# sourceMappingURL=chunk-YLPT5TUM.cjs.map