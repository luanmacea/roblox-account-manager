import { afterEach, describe, expect, it } from "vitest";
import type { PlatformCapabilities } from "../types";
import { isWindowsPlatform } from "./platform";

function caps(os: string): PlatformCapabilities {
  return {
    os,
    sessionType: "",
    preferredRunner: "",
    detectedRunner: "",
    runnerPath: null,
    supportsSingleLaunch: true,
    supportsMultiLaunch: true,
    supportsWatcher: true,
    supportsWatcherMemory: true,
    supportsWindowControls: true,
    supportsBotting: true,
    supportsUpdater: true,
    supportsClientSettings: true,
    supportsLiveAudio: false,
    supportsMemoryTrim: false,
    reasons: [],
    warnings: [],
  };
}

const originalUserAgent = navigator.userAgent;

function setUserAgent(value: string) {
  Object.defineProperty(navigator, "userAgent", { value, configurable: true });
}

afterEach(() => {
  setUserAgent(originalUserAgent);
});

describe("isWindowsPlatform", () => {
  it("trusts the reported capabilities first", () => {
    setUserAgent("Mozilla/5.0 (X11; Linux x86_64)");
    expect(isWindowsPlatform(caps("windows"))).toBe(true);
  });

  it("is false for any other reported OS, even on a Windows user agent", () => {
    setUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    expect(isWindowsPlatform(caps("linux"))).toBe(false);
    expect(isWindowsPlatform(caps("macos"))).toBe(false);
  });

  it("falls back to the user agent when capabilities are missing or unknown", () => {
    setUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    expect(isWindowsPlatform(null)).toBe(true);
    expect(isWindowsPlatform(caps(""))).toBe(true);

    setUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)");
    expect(isWindowsPlatform(null)).toBe(false);
  });

  it("matches the user agent case-insensitively", () => {
    setUserAgent("SOMETHING WINDOWS SOMETHING");
    expect(isWindowsPlatform(null)).toBe(true);
  });
});
