"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/modal.tsx
var _reactdialog = require('@radix-ui/react-dialog'); var Dialog = _interopRequireWildcard(_reactdialog);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function Modal({ open, onOpenChange, children, portalContainer, className }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, Dialog.Portal, { container: _nullishCoalesce(portalContainer, () => ( void 0)), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/40 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      Dialog.Content,
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0,
          "fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100dvh-2rem-env(safe-area-inset-top)-env(safe-area-inset-bottom))] w-[calc(100vw-2rem)] max-w-[440px] -translate-x-1/2 -translate-y-1/2 flex-col overscroll-contain",
          "rounded-xl bg-surface p-6 shadow-overlay outline-none",
          "data-[state=open]:animate-modal-in data-[state=closed]:animate-modal-out",
          className
        ),
        children
      }
    )
  ] }) });
}
function ModalHeader({
  title,
  description,
  onClose,
  className
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex items-start justify-between gap-4", className), children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-col gap-1.5", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Title, { className: "text-lg font-bold text-foreground", children: title }),
      description && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Description, { asChild: true, children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "text-sm leading-relaxed text-muted-foreground", children: description }) })
    ] }),
    onClose && /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      "button",
      {
        type: "button",
        onClick: onClose,
        "aria-label": "Close",
        className: "-mt-1 -mr-1 inline-flex size-11 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-surface-sunken",
        children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.X, { className: "size-4" })
      }
    )
  ] });
}
function ModalBody({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "lx-scroll mt-4 flex-1 overflow-y-auto", className), ...props });
}
function ModalFooter({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "mt-6 flex items-center justify-end gap-3", className), ...props });
}






exports.Modal = Modal; exports.ModalHeader = ModalHeader; exports.ModalBody = ModalBody; exports.ModalFooter = ModalFooter;
//# sourceMappingURL=chunk-YXKDAUEG.cjs.map