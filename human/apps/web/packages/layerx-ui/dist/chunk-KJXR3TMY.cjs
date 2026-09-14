"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/sheet.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _reactdialog = require('@radix-ui/react-dialog'); var Dialog = _interopRequireWildcard(_reactdialog);
var _jsxruntime = require('react/jsx-runtime');
function Sheet({ open, onOpenChange, children, portalContainer }) {
  const dragStartY = React.useRef(null);
  const startDrag = (event) => {
    if (!(event.target instanceof Element) || event.target.closest("[data-sheet-drag-handle]") === null) {
      return;
    }
    dragStartY.current = event.clientY;
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const finishDrag = (event) => {
    const startY = dragStartY.current;
    dragStartY.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (startY !== null && event.clientY - startY >= 72) {
      onOpenChange(false);
    }
  };
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, Dialog.Portal, { container: _nullishCoalesce(portalContainer, () => ( void 0)), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/40 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      Dialog.Content,
      {
        onPointerDown: startDrag,
        onPointerUp: finishDrag,
        onPointerCancel: () => {
          dragStartY.current = null;
        },
        className: _chunkMD6ORKN4cjs.cn.call(void 0,
          "fixed inset-x-0 bottom-0 z-50 mx-auto flex max-h-[calc(100dvh-env(safe-area-inset-top))] w-full max-w-lg flex-col overscroll-contain",
          "rounded-t-sheet bg-surface shadow-overlay outline-none",
          "data-[state=open]:animate-sheet-up data-[state=closed]:animate-sheet-down"
        ),
        children
      }
    )
  ] }) });
}
function SheetHeader({
  title,
  className,
  children
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col items-stretch", className), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex justify-center pt-2.5 pb-1", "aria-hidden": true, children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      "span",
      {
        "data-sheet-drag-handle": true,
        className: "h-5 w-12 touch-none rounded-full before:mx-auto before:mt-2 before:block before:h-1 before:w-10 before:rounded-full before:bg-border-strong"
      }
    ) }),
    (title || children) && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "border-b border-border px-5 pt-2 pb-4", children: [
      title && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Title, { className: "text-lg font-bold text-foreground", children: title }),
      children
    ] })
  ] });
}
function SheetDescription({
  className,
  ...props
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Description, { asChild: true, children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "p",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0, "text-[15px] leading-relaxed text-foreground-secondary", className),
      ...props
    }
  ) });
}
function SheetBody({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "lx-scroll flex-1 overflow-y-auto px-5 pt-4 pb-[max(1rem,env(safe-area-inset-bottom))]",
        className
      ),
      ...props
    }
  );
}
function SheetFooter({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "grid auto-cols-fr grid-flow-col gap-3 border-t border-border/0 px-5 pt-2 pb-[max(1.5rem,env(safe-area-inset-bottom))]",
        className
      ),
      ...props
    }
  );
}







exports.Sheet = Sheet; exports.SheetHeader = SheetHeader; exports.SheetDescription = SheetDescription; exports.SheetBody = SheetBody; exports.SheetFooter = SheetFooter;
//# sourceMappingURL=chunk-KJXR3TMY.cjs.map