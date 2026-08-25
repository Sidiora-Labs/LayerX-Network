export class MirrorOverloadedError extends Error {
  public constructor() {
    super("mirror verifier overloaded");
    this.name = "MirrorOverloadedError";
  }
}

export class MirrorVerificationAdmission {
  #active = 0;

  public constructor(readonly limit: number) {
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 32) {
      throw new Error("Mirror verification concurrency is invalid");
    }
  }

  public async run<T>(operation: () => Promise<T>): Promise<T> {
    if (this.#active >= this.limit) throw new MirrorOverloadedError();
    this.#active += 1;
    try {
      return await operation();
    } finally {
      this.#active -= 1;
    }
  }

  public active(): number {
    return this.#active;
  }
}
