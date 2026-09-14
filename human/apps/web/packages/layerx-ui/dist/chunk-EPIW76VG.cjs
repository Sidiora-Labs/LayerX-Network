"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";





var _chunkYXKDAUEGcjs = require('./chunk-YXKDAUEG.cjs');






var _chunkKJXR3TMYcjs = require('./chunk-KJXR3TMY.cjs');


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/responsive-dialog.tsx
var _reactdialog = require('@radix-ui/react-dialog'); var Dialog = _interopRequireWildcard(_reactdialog);
var _reactvisuallyhidden = require('@radix-ui/react-visually-hidden');
var _jsxruntime = require('react/jsx-runtime');
function ResponsiveDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  platform,
  portalContainer
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.Sheet, { open, onOpenChange, portalContainer, children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetHeader, { title }),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.SheetBody, { className: "flex flex-col gap-4", children: [
        description && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetDescription, { children: description }),
        children
      ] }),
      footer && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetFooter, { children: footer })
    ] });
  }
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkYXKDAUEGcjs.Modal, { open, onOpenChange, portalContainer, children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkYXKDAUEGcjs.ModalHeader, { title, description }),
    children && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkYXKDAUEGcjs.ModalBody, { children }),
    footer && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkYXKDAUEGcjs.ModalFooter, { children: footer })
  ] });
}
function ConfirmDialog({
  open,
  onOpenChange,
  icon,
  title,
  consequence,
  confirm,
  cancel,
  platform,
  portalContainer
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  const footer = /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _jsxruntime.Fragment, { children: [
    cancel && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      _chunkRA3A4XJ2cjs.Button,
      {
        variant: _nullishCoalesce(cancel.variant, () => ( "secondary")),
        size: "lg",
        fullWidth: resolved === "mobile",
        loading: cancel.loading,
        onClick: _nullishCoalesce(cancel.onClick, () => ( (() => onOpenChange(false)))),
        children: cancel.label
      }
    ),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
      _chunkRA3A4XJ2cjs.Button,
      {
        variant: _nullishCoalesce(confirm.variant, () => ( "primary")),
        size: "lg",
        fullWidth: resolved === "mobile",
        loading: confirm.loading,
        onClick: _nullishCoalesce(confirm.onClick, () => ( (() => onOpenChange(false)))),
        children: confirm.label
      }
    )
  ] });
  const body = /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col items-center gap-3 text-center", resolved === "desktop" && "py-2"), children: [
    icon && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex size-16 items-center justify-center rounded-full bg-surface-sunken text-foreground-secondary [&_svg]:size-7", children: icon }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "text-[15px] leading-relaxed text-foreground-secondary", children: consequence })
  ] });
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.Sheet, { open, onOpenChange, portalContainer, children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _reactvisuallyhidden.VisuallyHidden, { children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Title, { children: title }) }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetHeader, {}),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkKJXR3TMYcjs.SheetBody, { className: "flex flex-col gap-4 pt-2", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h2", { className: "text-center text-xl font-bold text-foreground", "aria-hidden": true, children: title }),
        body
      ] }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkKJXR3TMYcjs.SheetFooter, { children: footer })
    ] });
  }
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _chunkYXKDAUEGcjs.Modal, { open, onOpenChange, portalContainer, children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _reactvisuallyhidden.VisuallyHidden, { children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Title, { children: title }) }),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-col gap-4", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h2", { className: "text-center text-xl font-bold text-foreground", "aria-hidden": true, children: title }),
      body,
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "mt-2 grid auto-cols-fr grid-flow-col gap-3", children: footer })
    ] })
  ] });
}




exports.ResponsiveDialog = ResponsiveDialog; exports.ConfirmDialog = ConfirmDialog;
//# sourceMappingURL=chunk-EPIW76VG.cjs.map