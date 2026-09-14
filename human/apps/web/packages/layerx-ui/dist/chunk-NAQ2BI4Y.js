"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/option-list.tsx
import * as RadioGroup from "@radix-ui/react-radio-group";
import { jsx, jsxs } from "react/jsx-runtime";
function OptionList({
  items,
  value,
  onValueChange,
  className,
  "aria-label": ariaLabel
}) {
  return /* @__PURE__ */ jsx(
    RadioGroup.Root,
    {
      value,
      onValueChange,
      className: cn("flex flex-col divide-y divide-border/70", className),
      "aria-label": ariaLabel,
      children: items.map((item) => {
        const checked = item.value === value;
        return /* @__PURE__ */ jsxs(
          RadioGroup.Item,
          {
            value: item.value,
            className: "group flex w-full cursor-pointer items-center justify-between gap-3 py-4 text-left outline-none",
            children: [
              /* @__PURE__ */ jsxs("span", { className: "flex min-w-0 flex-col gap-0.5", children: [
                /* @__PURE__ */ jsx("span", { className: "text-[15px] font-medium text-foreground", children: item.label }),
                item.description && /* @__PURE__ */ jsx("span", { className: "text-[13px] text-muted-foreground", children: item.description })
              ] }),
              /* @__PURE__ */ jsx(
                "span",
                {
                  className: cn(
                    "inline-flex size-[22px] shrink-0 items-center justify-center rounded-full border-2 transition-colors",
                    checked ? "border-accent" : "border-border-strong group-hover:border-faint-foreground"
                  ),
                  "aria-hidden": true,
                  children: checked && /* @__PURE__ */ jsx("span", { className: "size-3 rounded-full bg-accent" })
                }
              )
            ]
          },
          item.value
        );
      })
    }
  );
}

export {
  OptionList
};
//# sourceMappingURL=chunk-NAQ2BI4Y.js.map