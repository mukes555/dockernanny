import { describe, expect, it } from "vitest";

import { localPort, parseLogLine } from "./types";
import type { Stack } from "./types";

describe("parseLogLine", () => {
  it("splits compose's container prefix from the text", () => {
    expect(parseLogLine("shop-db-1  | ready to accept connections")).toEqual({ container: "shop-db-1", text: "ready to accept connections" });
  });

  it("keeps a line without a prefix whole", () => {
    expect(parseLogLine("no prefix here")).toEqual({ container: "", text: "no prefix here" });
  });

  it("splits at the first separator only", () => {
    expect(parseLogLine("api | GET / | 200")).toEqual({ container: "api", text: "GET / | 200" });
  });
});

describe("localPort", () => {
  const stack: Stack = {
    id: "s",
    name: "shop",
    machine_id: "m",
    project_dir: "/p",
    compose_rel: "compose.yaml",
    excludes: [],
    forward_ports: true,
    live_sync: false,
    port_overrides: { "5432": 6432 },
  };

  it("follows the stack's override, and the published port otherwise", () => {
    expect(localPort(stack, 5432)).toBe(6432);
    expect(localPort(stack, 3000)).toBe(3000);
  });
});
