export type ArrayElement<ArrType> = ArrType extends ReadonlyArray<infer ElementType> ?
  ElementType :
  never;

export type ExcludeNull<T> = T extends null ? never : T;

export type ExcludeUndefined<T> = T extends undefined ? never : T;

export type KeysOfObjectOrNull<T> = keyof ExcludeNull<T>;

/** Combines members of an intersection into a readable type. */
// https://twitter.com/mattpocockuk/status/1622730173446557697?s=20&t=NdpAcmEFXY01xkqU3KO0Mg
export type Evaluate<Type> = { [key in keyof Type]: Type[key] } & unknown;

// Keeps in the object type only those properties that have the provided type (e.g only numbers)
export type PickByType<T, X> = Record<
  { [K in keyof T]: T[K] extends X ? K : never }[keyof T],
  X
>;

// Make some properties of an object optional
export type PartialBy<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;
