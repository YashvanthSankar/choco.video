// Platform words for the UI. The engine reports `hardware.os` ("macos" or "windows"); the page sets
// it once after the first status call and every component reads from here.
export const platform = $state({ os: "macos" });

export const words = {
  get isMac() { return platform.os === "macos"; },
  /** "Mac" / "PC" — as in "Your video stays on this Mac". */
  get machine() { return platform.os === "macos" ? "Mac" : "PC"; },
  /** Where a file is revealed. */
  get reveal() { return platform.os === "macos" ? "Show in Finder" : "Show in Explorer"; },
  /** Where the provider key is stored. */
  get vault() { return platform.os === "macos" ? "Keychain" : "Windows Credential Manager"; },
  get osName() { return platform.os === "macos" ? "macOS" : "Windows"; },
};
