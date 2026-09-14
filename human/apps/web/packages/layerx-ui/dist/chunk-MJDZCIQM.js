"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/sheet.tsx
import * as React from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { jsx, jsxs } from "react/jsx-runtime";
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
  return /* @__PURE__ */ jsx(Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ jsxs(Dialog.Portal, { container: portalContainer ?? void 0, children: [
    /* @__PURE__ */ jsx(Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/40 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" }),
    /* @__PURE__ */ jsx(
      Dialog.Content,
      {
        onPointerDown: startDrag,
        onPointerUp: finishDrag,
        onPointerCancel: () => {
          dragStartY.current = null;
        },
        className: cn(
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
  return /* @__PURE__ */ jsxs("div", { className: cn("flex flex-col items-stretch", className), children: [
    /* @__PURE__ */ jsx("div", { className: "flex justify-center pt-2.5 pb-1", "aria-hidden": true, children: /* @__PURE__ */ jsx(
      "span",
      {
        "data-sheet-drag-handle": true,
        className: "h-5 w-12 touch-none rounded-full before:mx-auto before:mt-2 before:block before:h-1 before:w-10 before:rounded-full before:bg-border-strong"
      }
    ) }),
    (title || children) && /* @__PURE__ */ jsxs("div", { className: "border-b border-border px-5 pt-2 pb-4", children: [
      title && /* @__PURE__ */ jsx(Dialog.Title, { className: "text-lg font-bold text-foreground", children: title }),
      children
    ] })
  ] });
}
function SheetDescription({
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(Dialog.Description, { asChild: true, children: /* @__PURE__ */ jsx(
    "p",
    {
      className: cn("text-[15px] leading-relaxed text-foreground-secondary", className),
      ...props
    }
  ) });
}
function SheetBody({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: cn(
        "lx-scroll flex-1 overflow-y-auto px-5 pt-4 pb-[max(1rem,env(safe-area-inset-bottom))]",
        className
      ),
      ...props
    }
  );
}
function SheetFooter({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: cn(
        "grid auto-cols-fr grid-flow-col gap-3 border-t border-border/0 px-5 pt-2 pb-[max(1.5rem,env(safe-area-inset-bottom))]",
        className
      ),
      ...props
    }
  );
}

export {
  Sheet,
  SheetHeader,
  SheetDescription,
  SheetBody,
  SheetFooter
};
//# sourceMappingURL=chunk-MJDZCIQM.js.map