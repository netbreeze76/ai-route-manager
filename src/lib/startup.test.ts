import { describe, expect, it } from "vitest";
import { settleTask } from "./startup";

describe("settleTask", () => {
  it("keeps a successful startup result", async () => {
    await expect(settleTask(Promise.resolve(42))).resolves.toEqual({ ok: true, value: 42 });
  });

  it("isolates a failed startup task", async () => {
    await expect(settleTask(Promise.reject(new Error("broken preferences")))).resolves.toEqual({
      ok: false,
      error: "broken preferences",
    });
  });
});
