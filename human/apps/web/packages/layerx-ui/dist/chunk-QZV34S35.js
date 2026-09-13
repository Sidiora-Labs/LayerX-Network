"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/drawer.tsx
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function Drawer({ open, onOpenChange, children, portalContainer, width = 420 }) {
  return /* @__PURE__ */ jsx(Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ jsxs(Dialog.Portal, { container: portalContainer ?? void 0, children: [
    /* @__PURE__ */ jsx(Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/30 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" }),
    /* @__PURE__ */ jsx(
      Dialog.Content,
      {
        style: { width: `min(${typeof width === "number" ? `${width}px` : width}, 100vw)` },
        className: cn(
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
  return /* @__PURE__ */ jsxs("div", { className: cn("flex items-start justify-between gap-4 border-b border-border p-5", className), children: [
    /* @__PURE__ */ jsxs("div", { className: "flex flex-col gap-1", children: [
      /* @__PURE__ */ jsx(Dialog.Title, { className: "text-lg font-bold text-foreground", children: title }),
      description && /* @__PURE__ */ jsx(Dialog.Description, { asChild: true, children: /* @__PURE__ */ jsx("p", { className: "text-sm text-muted-foreground", children: description }) })
    ] }),
    onClose && /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        onClick: onClose,
        "aria-label": "Close",
        className: "inline-flex size-11 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-surface-sunken",
        children: /* @__PURE__ */ jsx(X, { className: "size-4" })
      }
    )
  ] });
}
function DrawerBody({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: cn("lx-scroll flex-1 overflow-y-auto p-5", className), ...props });
}
function DrawerFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: cn("flex items-center justify-end gap-3 border-t border-border p-4", className), ...props });
}

export {
  Drawer,
  DrawerHeader,
  DrawerBody,
  DrawerFooter
};
//# sourceMappingURL=chunk-QZV34S35.js.map