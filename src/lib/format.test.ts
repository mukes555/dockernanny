import { describe, expect, it } from "vitest";

import { ago, byteSize, memoryGb, plural, span, stopwatch } from "./format";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

describe("format", () => {
  it("says a span the way a person does", () => {
    expect(span(0)).toBe("0m");
    expect(span(59 * MINUTE)).toBe("59m");
    expect(span(65 * MINUTE)).toBe("1h 5m");
    expect(span(28 * HOUR)).toBe("1d 4h");
    expect(span(-5 * MINUTE)).toBe("0m");
  });

  it("says how long ago in the largest whole unit", () => {
    expect(ago(12_000)).toBe("12s ago");
    expect(ago(3 * MINUTE)).toBe("3m ago");
    expect(ago(2 * HOUR)).toBe("2h ago");
    expect(ago(-1_000)).toBe("0s ago");
  });

  it("runs a stopwatch with padded seconds", () => {
    expect(stopwatch(45_000)).toBe("45s");
    expect(stopwatch(187_000)).toBe("3m 07s");
  });

  it("shows sizes in MB below a gigabyte and whole GB from a hundred", () => {
    expect(byteSize(640e6)).toBe("640 MB");
    expect(byteSize(1.2e9)).toBe("1.2 GB");
    expect(byteSize(250e9)).toBe("250 GB");
    expect(memoryGb(16_000)).toBe("15.6");
  });

  it("counts with the right word", () => {
    expect(plural(1, "stack")).toBe("1 stack");
    expect(plural(3, "stack")).toBe("3 stacks");
    expect(plural(2, "copy", "copies")).toBe("2 copies");
  });
});
