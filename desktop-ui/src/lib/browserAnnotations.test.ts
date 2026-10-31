import { expect, test } from "bun:test";
import fixture from "./fixtures/browser-annotations.json";
import { expected } from "./fixtures/browserAnnotations";


test("v2 snapshot wire fixture matches the desktop contract", () => {
  expect(fixture).toEqual(expected);
});
