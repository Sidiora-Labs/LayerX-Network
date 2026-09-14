"use client";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  Button
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/primary-action.tsx
import { jsx } from "react/jsx-runtime";
function PrimaryAction({
  children,
  platform,
  position = "footer",
  className,
  ...props
}) {
  const resolved = usePlatform(platform);
  if (resolved === "mobile") {
    return /* @__PURE__ */ jsx(
      "div",
      {
        className: cn(
          "sticky bottom-0 z-20 -mx-4 mt-auto bg-[linear-gradient(to_top,var(--background)_60%,transparent)] px-4 pt-6 pb-[max(1rem,env(safe-area-inset-bottom))]"
        ),
        children: /* @__PURE__ */ jsx(Button, { size: "lg", fullWidth: true, className, ...props, children })
      }
    );
  }
  return /* @__PURE__ */ jsx(
    Button,
    {
      size: position === "header" ? "md" : "lg",
      className: cn(position === "footer" && "min-w-[180px]", className),
      ...props,
      children
    }
  );
}

export {
  PrimaryAction
};
//# sourceMappingURL=chunk-HMBLSESK.js.map