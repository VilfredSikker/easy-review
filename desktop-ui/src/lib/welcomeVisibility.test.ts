import { describe, expect, it } from "bun:test";
import { showFullWelcome, type WelcomeInputs } from "./welcomeVisibility";

const noRepo: WelcomeInputs = {
  naturalEmpty: true,
  explicitFullWelcome: false,
  loading: false,
  mainView: "diff",
};

describe("showFullWelcome", () => {
  it("shows the welcome when no repo is open", () => {
    expect(showFullWelcome(noRepo)).toBe(true);
  });

  it("gives way to settings opened from the welcome", () => {
    // "App settings" and "Uninstall" switch to the settings view; with no repo
    // the welcome used to stay on top, so both buttons did nothing.
    expect(showFullWelcome({ ...noRepo, mainView: "settings" })).toBe(false);
  });

  it("stays hidden while loading", () => {
    expect(showFullWelcome({ ...noRepo, loading: true })).toBe(false);
  });

  it("stays hidden when a repo is open and the user did not ask for it", () => {
    expect(showFullWelcome({ ...noRepo, naturalEmpty: false })).toBe(false);
  });
});
