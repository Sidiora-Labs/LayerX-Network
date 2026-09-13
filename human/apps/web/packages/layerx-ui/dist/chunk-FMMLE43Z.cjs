"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/card.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _classvarianceauthority = require('class-variance-authority');
var _jsxruntime = require('react/jsx-runtime');
var cardVariants = _classvarianceauthority.cva.call(void 0, "rounded-lg bg-surface", {
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
  ({ className, elevation, padding, ...props }, ref) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { ref, className: _chunkMD6ORKN4cjs.cn.call(void 0, cardVariants({ elevation, padding, className })), ...props })
);
Card.displayName = "Card";




exports.cardVariants = cardVariants; exports.Card = Card;
//# sourceMappingURL=chunk-FMMLE43Z.cjs.map