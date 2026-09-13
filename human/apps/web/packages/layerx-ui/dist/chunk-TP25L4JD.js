"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/modal.tsx
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function Modal({ open, onOpenChange, children, portalContainer, className }) {
  return /* @__PURE__ */ jsx(Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ jsxs(Dialog.Portal, { container: portalContainer ?? void 0, children: [
    /* @__PURE__ */ jsx(Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/40 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" }),
    /* @__PURE__ */ jsx(
      Dialog.Content,
      {
        className: cn(
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
  return /* @__PURE__ */ jsxs("div", { className: cn("flex items-start justify-between gap-4", className), children: [
    /* @__PURE__ */ jsxs("div", { className: "flex flex-col gap-1.5", children: [
      /* @__PURE__ */ jsx(Dialog.Title, { className: "text-lg font-bold text-foreground", children: title }),
      description && /* @__PURE__ */ jsx(Dialog.Description, { asChild: true, children: /* @__PURE__ */ jsx("p", { className: "text-sm leading-relaxed text-muted-foreground", children: description }) })
    ] }),
    onClose && /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        onClick: onClose,
        "aria-label": "Close",
        className: "-mt-1 -mr-1 inline-flex size-11 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-surface-sunken",
        children: /* @__PURE__ */ jsx(X, { className: "size-4" })
      }
    )
  ] });
}
function ModalBody({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: cn("lx-scroll mt-4 flex-1 overflow-y-auto", className), ...props });
}
function ModalFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx("div", { className: cn("mt-6 flex items-center justify-end gap-3", className), ...props });
}

export {
  Modal,
  ModalHeader,
  ModalBody,
  ModalFooter
};
//# sourceMappingURL=chunk-TP25L4JD.js.map