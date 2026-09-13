import { copyEntry } from "./runtime.ts";

export type CopyValue = string | number;
export type CopyValues = Readonly<Record<string, CopyValue>>;

function matchingBrace(message: string, opening: number): number {
  let depth = 0;
  for (let index = opening; index < message.length; index += 1) {
    if (message[index] === "{") {
      depth += 1;
    } else if (message[index] === "}") {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }
  throw new Error("Unclosed ICU argument");
}

function splitDirective(directive: string): readonly [string, string | undefined, string | undefined] {
  const first = directive.indexOf(",");
  if (first < 0) {
    return [directive.trim(), undefined, undefined];
  }
  const second = directive.indexOf(",", first + 1);
  if (second < 0) {
    return [directive.slice(0, first).trim(), directive.slice(first + 1).trim(), undefined];
  }
  return [
    directive.slice(0, first).trim(),
    directive.slice(first + 1, second).trim(),
    directive.slice(second + 1).trim(),
  ];
}

function options(source: string): ReadonlyMap<string, string> {
  const parsed = new Map<string, string>();
  let cursor = 0;
  while (cursor < source.length) {
    while (source[cursor] === " ") {
      cursor += 1;
    }
    const brace = source.indexOf("{", cursor);
    if (brace < 0) {
      break;
    }
    const key = source.slice(cursor, brace).trim();
    const closing = matchingBrace(source, brace);
    parsed.set(key, source.slice(brace + 1, closing));
    cursor = closing + 1;
  }
  return parsed;
}

function renderDirective(directive: string, values: CopyValues): string {
  const [name, kind, optionSource] = splitDirective(directive);
  const value = values[name];
  if (value === undefined) {
    throw new Error(`Missing copy value: ${name}`);
  }
  if (kind === undefined) {
    return String(value);
  }
  if (optionSource === undefined || (kind !== "plural" && kind !== "select")) {
    throw new Error(`Unsupported ICU directive: ${kind}`);
  }
  const choices = options(optionSource);
  if (kind === "select") {
    const selection = choices.get(String(value)) ?? choices.get("other");
    if (selection === undefined) {
      throw new Error(`ICU select has no branch for ${name}`);
    }
    return renderMessage(selection, values);
  }
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`ICU plural value is not a finite number: ${name}`);
  }
  const exact = choices.get(`=${String(value)}`);
  const category = new Intl.PluralRules("en").select(value);
  const selection = exact ?? choices.get(category) ?? choices.get("other");
  if (selection === undefined) {
    throw new Error(`ICU plural has no branch for ${name}`);
  }
  return renderMessage(selection.replaceAll("#", String(value)), values);
}

export function renderMessage(message: string, values: CopyValues): string {
  let rendered = "";
  let cursor = 0;
  while (cursor < message.length) {
    const opening = message.indexOf("{", cursor);
    if (opening < 0) {
      rendered = `${rendered}${message.slice(cursor)}`;
      break;
    }
    rendered = `${rendered}${message.slice(cursor, opening)}`;
    const closing = matchingBrace(message, opening);
    rendered = `${rendered}${renderDirective(message.slice(opening + 1, closing), values)}`;
    cursor = closing + 1;
  }
  return rendered;
}

export function formatCopy(key: string, values: CopyValues = {}): string {
  return renderMessage(copyEntry(key).message, values);
}
