import { describe, it, expect } from "../conformance-runner/node-test.js";
import { ResumableStream, StreamPage, StreamCursor, streamCursor, PlatformSdkError } from "@sidiora/layerx-sdk";

describe("StreamCursor hygiene", () => {
  it("constructs valid cursors", () => {
    const cursor = streamCursor("cursor-123");
    expect(cursor).toBeTruthy();
  });

  it("refuses empty cursors", () => {
    expect(() => streamCursor("")).toThrow(PlatformSdkError);
  });

  it("refuses overlong cursors", () => {
    const overlong = "a".repeat(513);
    expect(() => streamCursor(overlong)).toThrow(PlatformSdkError);
  });

  it("refuses NUL-containing cursors", () => {
    expect(() => streamCursor("has\0null")).toThrow(PlatformSdkError);
  });
});

describe("ResumableStream no-gap-no-duplicate semantics", () => {
  it("accepts an empty page", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [],
      nextCursor: initialCursor,
    };
    const accepted = stream.accept(page);
    expect(accepted).toHaveLength(0);
    expect(stream.cursor).toBe(initialCursor);
  });

  it("accepts a single event", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const nextCursor = streamCursor("c1");
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: initialCursor,
          cursor: nextCursor,
          value: "first",
        },
      ],
      nextCursor,
    };
    const accepted = stream.accept(page);
    expect(accepted).toHaveLength(1);
    expect(accepted[0]?.value).toBe("first");
    expect(stream.cursor).toBe(nextCursor);
  });

  it("accepts multiple events forming a chain", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const c2 = streamCursor("c2");
    const c3 = streamCursor("c3");
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: initialCursor,
          cursor: c1,
          value: "first",
        },
        {
          eventId: "e2",
          previousCursor: c1,
          cursor: c2,
          value: "second",
        },
        {
          eventId: "e3",
          previousCursor: c2,
          cursor: c3,
          value: "third",
        },
      ],
      nextCursor: c3,
    };
    const accepted = stream.accept(page);
    expect(accepted).toHaveLength(3);
    expect(stream.cursor).toBe(c3);
  });

  it("refuses a page with wrong requestedCursor", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const wrongCursor = streamCursor("c-wrong");
    const page: StreamPage<string> = {
      requestedCursor: wrongCursor,
      events: [],
      nextCursor: wrongCursor,
    };
    expect(() => stream.accept(page)).toThrow(PlatformSdkError);
  });

  it("refuses a gap in the cursor chain", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const c2 = streamCursor("c2");
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: c1,
          cursor: c2,
          value: "skipped",
        },
      ],
      nextCursor: c2,
    };
    expect(() => stream.accept(page)).toThrow(PlatformSdkError);
  });

  it("refuses duplicate eventIds across pages", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const c2 = streamCursor("c2");
    const firstPage: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: initialCursor,
          cursor: c1,
          value: "first",
        },
      ],
      nextCursor: c1,
    };
    stream.accept(firstPage);
    const duplicatePage: StreamPage<string> = {
      requestedCursor: c1,
      events: [
        {
          eventId: "e1",
          previousCursor: c1,
          cursor: c2,
          value: "duplicate",
        },
      ],
      nextCursor: c2,
    };
    expect(() => stream.accept(duplicatePage)).toThrow(PlatformSdkError);
  });

  it("refuses duplicate eventIds within a single page", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const c2 = streamCursor("c2");
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: initialCursor,
          cursor: c1,
          value: "first",
        },
        {
          eventId: "e1",
          previousCursor: c1,
          cursor: c2,
          value: "duplicate",
        },
      ],
      nextCursor: c2,
    };
    expect(() => stream.accept(page)).toThrow(PlatformSdkError);
  });

  it("refuses empty eventId", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "",
          previousCursor: initialCursor,
          cursor: c1,
          value: "no-id",
        },
      ],
      nextCursor: c1,
    };
    expect(() => stream.accept(page)).toThrow(PlatformSdkError);
  });

  it("refuses mismatched nextCursor", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const c2 = streamCursor("c2");
    const page: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: initialCursor,
          cursor: c1,
          value: "first",
        },
      ],
      nextCursor: c2,
    };
    expect(() => stream.accept(page)).toThrow(PlatformSdkError);
  });

  it("supports resumption after disconnection", () => {
    const initialCursor = streamCursor("c0");
    const stream = new ResumableStream(initialCursor);
    const c1 = streamCursor("c1");
    const c2 = streamCursor("c2");
    const firstPage: StreamPage<string> = {
      requestedCursor: initialCursor,
      events: [
        {
          eventId: "e1",
          previousCursor: initialCursor,
          cursor: c1,
          value: "before-disconnect",
        },
      ],
      nextCursor: c1,
    };
    stream.accept(firstPage);
    const resumedPage: StreamPage<string> = {
      requestedCursor: c1,
      events: [
        {
          eventId: "e2",
          previousCursor: c1,
          cursor: c2,
          value: "after-reconnect",
        },
      ],
      nextCursor: c2,
    };
    const accepted = stream.accept(resumedPage);
    expect(accepted).toHaveLength(1);
    expect(accepted[0]?.value).toBe("after-reconnect");
    expect(stream.cursor).toBe(c2);
  });
});
