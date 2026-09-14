"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/avatar.tsx
import * as React from "react";
import { cva } from "class-variance-authority";
import { jsx } from "react/jsx-runtime";
var avatarVariants = cva(
  "relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full font-semibold select-none",
  {
    variants: {
      size: {
        xs: "size-7 text-[11px]",
        sm: "size-9 text-xs",
        md: "size-11 text-sm",
        lg: "size-14 text-base",
        xl: "size-20 text-xl"
      },
      tone: {
        /** Black tile with white initials — the design set's profile avatar. */
        primary: "bg-primary text-primary-foreground",
        accent: "bg-accent-soft text-accent-strong",
        neutral: "bg-surface-sunken text-foreground-secondary"
      }
    },
    defaultVariants: { size: "md", tone: "neutral" }
  }
);
function deriveInitials(name) {
  if (!name) return "";
  return name.split(" ").filter(Boolean).slice(0, 2).map((p) => p[0].toUpperCase()).join("");
}
var Avatar = React.forwardRef(
  ({ className, size, tone, src, alt, initials, ...props }, ref) => {
    const [imgFailed, setImgFailed] = React.useState(false);
    const showImage = src && !imgFailed;
    return /* @__PURE__ */ jsx("span", { ref, className: cn(avatarVariants({ size, tone, className })), ...props, children: showImage ? (
      // eslint-disable-next-line @next/next/no-img-element
      /* @__PURE__ */ jsx(
        "img",
        {
          src,
          alt: alt ?? "",
          className: "absolute inset-0 size-full object-cover",
          onError: () => setImgFailed(true)
        }
      )
    ) : /* @__PURE__ */ jsx("span", { "aria-hidden": true, children: initials ?? deriveInitials(alt) }) });
  }
);
Avatar.displayName = "Avatar";

export {
  avatarVariants,
  Avatar
};
//# sourceMappingURL=chunk-7HP2I7K3.js.map