"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/drawer.tsx
var _reactdialog = require('@radix-ui/react-dialog'); var Dialog = _interopRequireWildcard(_reactdialog);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function Drawer({ open, onOpenChange, children, portalContainer, width = 420 }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, Dialog.Portal, { container: _nullishCoalesce(portalContainer, () => ( void 0)), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/30 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      Dialog.Content,
      {
        style: { width: `min(${typeof width === "number" ? `${width}px` : width}, 100vw)` },
        className: _chunkMD6ORKN4cjs.cn.call(void 0,
          "fixed top-0 right-0 z-50 flex h-dvh flex-col overscroll-contain bg-surface pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)] shadow-overlay outline-none",
          "data-[state=open]:animate-drawer-in data-[state=closed]:animate-drawer-out"
        ),
        children
      }
    )
  ] }) });
}
function DrawerHeader({
  title,
  description,
  onClose,
  className
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex items-start justify-between gap-4 border-b border-border p-5", className), children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-col gap-1", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Title, { className: "text-lg font-bold text-foreground", children: title }),
      description && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Description, { asChild: true, children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "text-sm text-muted-foreground", children: description }) })
    ] }),
    onClose && /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      "button",
      {
        type: "button",
        onClick: onClose,
        "aria-label": "Close",
        className: "inline-flex size-11 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-surface-sunken",
        children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.X, { className: "size-4" })
      }
    )
  ] });
}
function DrawerBody({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "lx-scroll flex-1 overflow-y-auto p-5", className), ...props });
}
function DrawerFooter({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex items-center justify-end gap-3 border-t border-border p-4", className), ...props });
}






exports.Drawer = Drawer; exports.DrawerHeader = DrawerHeader; exports.DrawerBody = DrawerBody; exports.DrawerFooter = DrawerFooter;
//# sourceMappingURL=chunk-2CZ6QC47.cjs.map