import { deepStrictEqual, doesNotThrow, ok, strictEqual, throws } from "node:assert";
import { describe, it } from "node:test";

type ErrorConstructor = new (...args: never[]) => Error;

function expectation<T>(actual: T, negated: boolean) {
  const check = (condition: boolean, message: string): void => {
    if (negated ? condition : !condition) {
      throw new Error(message);
    }
  };
  return {
    toBe(expected: unknown): void {
      if (negated) {
        check(Object.is(actual, expected), "expected values not to be identical");
      } else {
        strictEqual(actual, expected);
      }
    },
    toBeTruthy(): void {
      check(Boolean(actual), "expected value to be truthy");
    },
    toContain(expected: unknown): void {
      const container = actual as { includes(value: unknown): boolean };
      check(container.includes(expected), "expected value to contain member");
    },
    toHaveLength(expected: number): void {
      const value = actual as { length: number };
      if (negated) {
        check(value.length === expected, `expected length not to be ${expected}`);
      } else {
        strictEqual(value.length, expected);
      }
    },
    toThrow(expected?: ErrorConstructor): void {
      const action = actual as () => unknown;
      if (negated) {
        doesNotThrow(action);
      } else if (expected === undefined) {
        throws(action);
      } else {
        throws(action, expected);
      }
    },
    toEqual(expected: unknown): void {
      if (negated) {
        ok(JSON.stringify(actual) !== JSON.stringify(expected));
      } else {
        deepStrictEqual(actual, expected);
      }
    },
  };
}

export function expect<T>(actual: T) {
  return {
    ...expectation(actual, false),
    not: expectation(actual, true),
  };
}

export { describe, it };
