import { describe, expect, it } from "vitest";
import { assertV1ServiceFlagAgreement, V1ServiceFlagMismatch } from "./v1ServiceFlag";

describe("v1 service flag agreement", () => {
  it("accepts matching builds", () => {
    expect(() => assertV1ServiceFlagAgreement(false, undefined)).not.toThrow();
    expect(() => assertV1ServiceFlagAgreement(false, "0")).not.toThrow();
    expect(() => assertV1ServiceFlagAgreement(true, "1")).not.toThrow();
  });

  it("refuses a frontend asking for the flag while the backend has it off", () => {
    expect(() => assertV1ServiceFlagAgreement(false, "1")).toThrow(
      V1ServiceFlagMismatch,
    );
  });

  it("refuses a backend on the flag path that the frontend was not built for", () => {
    expect(() => assertV1ServiceFlagAgreement(true, undefined)).toThrow(
      V1ServiceFlagMismatch,
    );
    expect(() => assertV1ServiceFlagAgreement(true, "0")).toThrow(
      V1ServiceFlagMismatch,
    );
  });
});
