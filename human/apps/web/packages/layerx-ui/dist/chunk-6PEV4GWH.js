"use client";
import {
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader
} from "./chunk-TP25L4JD.js";
import {
  Sheet,
  SheetBody,
  SheetDescription,
  SheetFooter,
  SheetHeader
} from "./chunk-MJDZCIQM.js";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  Button
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/responsive-dialog.tsx
import * as Dialog from "@radix-ui/react-dialog";
import { VisuallyHidden } from "@radix-ui/react-visually-hidden";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
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
  const resolved = usePlatform(platform);
  if (resolved === "mobile") {
    return /* @__PURE__ */ jsxs(Sheet, { open, onOpenChange, portalContainer, children: [
      /* @__PURE__ */ jsx(SheetHeader, { title }),
      /* @__PURE__ */ jsxs(SheetBody, { className: "flex flex-col gap-4", children: [
        description && /* @__PURE__ */ jsx(SheetDescription, { children: description }),
        children
      ] }),
      footer && /* @__PURE__ */ jsx(SheetFooter, { children: footer })
    ] });
  }
  return /* @__PURE__ */ jsxs(Modal, { open, onOpenChange, portalContainer, children: [
    /* @__PURE__ */ jsx(ModalHeader, { title, description }),
    children && /* @__PURE__ */ jsx(ModalBody, { children }),
    footer && /* @__PURE__ */ jsx(ModalFooter, { children: footer })
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
  const resolved = usePlatform(platform);
  const footer = /* @__PURE__ */ jsxs(Fragment, { children: [
    cancel && /* @__PURE__ */ jsx(
      Button,
      {
        variant: cancel.variant ?? "secondary",
        size: "lg",
        fullWidth: resolved === "mobile",
        loading: cancel.loading,
        onClick: cancel.onClick ?? (() => onOpenChange(false)),
        children: cancel.label
      }
    ),
    /* @__PURE__ */ jsx(
      Button,
      {
        variant: confirm.variant ?? "primary",
        size: "lg",
        fullWidth: resolved === "mobile",
        loading: confirm.loading,
        onClick: confirm.onClick ?? (() => onOpenChange(false)),
        children: confirm.label
      }
    )
  ] });
  const body = /* @__PURE__ */ jsxs("div", { className: cn("flex flex-col items-center gap-3 text-center", resolved === "desktop" && "py-2"), children: [
    icon && /* @__PURE__ */ jsx("span", { className: "inline-flex size-16 items-center justify-center rounded-full bg-surface-sunken text-foreground-secondary [&_svg]:size-7", children: icon }),
    /* @__PURE__ */ jsx("p", { className: "text-[15px] leading-relaxed text-foreground-secondary", children: consequence })
  ] });
  if (resolved === "mobile") {
    return /* @__PURE__ */ jsxs(Sheet, { open, onOpenChange, portalContainer, children: [
      /* @__PURE__ */ jsx(VisuallyHidden, { children: /* @__PURE__ */ jsx(Dialog.Title, { children: title }) }),
      /* @__PURE__ */ jsx(SheetHeader, {}),
      /* @__PURE__ */ jsxs(SheetBody, { className: "flex flex-col gap-4 pt-2", children: [
        /* @__PURE__ */ jsx("h2", { className: "text-center text-xl font-bold text-foreground", "aria-hidden": true, children: title }),
        body
      ] }),
      /* @__PURE__ */ jsx(SheetFooter, { children: footer })
    ] });
  }
  return /* @__PURE__ */ jsxs(Modal, { open, onOpenChange, portalContainer, children: [
    /* @__PURE__ */ jsx(VisuallyHidden, { children: /* @__PURE__ */ jsx(Dialog.Title, { children: title }) }),
    /* @__PURE__ */ jsxs("div", { className: "flex flex-col gap-4", children: [
      /* @__PURE__ */ jsx("h2", { className: "text-center text-xl font-bold text-foreground", "aria-hidden": true, children: title }),
      body,
      /* @__PURE__ */ jsx("div", { className: "mt-2 grid auto-cols-fr grid-flow-col gap-3", children: footer })
    ] })
  ] });
}

export {
  ResponsiveDialog,
  ConfirmDialog
};
//# sourceMappingURL=chunk-6PEV4GWH.js.map