"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/card.tsx
import * as React from "react";
import { cva } from "class-variance-authority";
import { jsx } from "react/jsx-runtime";
var cardVariants = cva("rounded-lg bg-surface", {
  variants: {
    elevation: {
      /** Hairline border + soft shadow — the design set's default card. */
      raised: "border border-border shadow-card",
      outline: "border border-border",
      flat: "bg-surface-sunken/60"
    },
    padding: {
      none: "",
      sm: "p-3",
      md: "p-4",
      lg: "p-5"
    }
  },
  defaultVariants: { elevation: "raised", padding: "md" }
});
var Card = React.forwardRef(
  ({ className, elevation, padding, ...props }, ref) => /* @__PURE__ */ jsx("div", { ref, className: cn(cardVariants({ elevation, padding, className })), ...props })
);
Card.displayName = "Card";

export {
  cardVariants,
  Card
};
//# sourceMappingURL=chunk-LOMXTVEB.js.map