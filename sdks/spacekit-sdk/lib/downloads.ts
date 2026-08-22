export type DetectedOS = "mac" | "windows" | "linux" | "unknown";

export const DOWNLOAD_URLS = {
  mac: "https://releases.spacekit.xyz/SpaceKit-Desktop-1.0.0-mac.dmg",
  windows: "https://releases.spacekit.xyz/SpaceKit-Desktop-1.0.0-windows.exe",
  linux: "https://releases.spacekit.xyz/SpaceKit-Desktop-1.0.0-linux.AppImage",
} as const;

export function detectOS(): DetectedOS {
  if (typeof navigator === "undefined") return "unknown";

  const userAgent = navigator.userAgent.toLowerCase();
  const platform = (navigator.platform || "").toLowerCase();

  if (platform.includes("mac") || userAgent.includes("mac")) return "mac";
  if (platform.includes("win") || userAgent.includes("windows")) return "windows";
  if (platform.includes("linux") || userAgent.includes("linux")) return "linux";

  return "unknown";
}

export function getPrimaryDownload(os: DetectedOS): {
  url?: string;
  buttonText: string;
  infoText: string;
} {
  if (os === "mac") {
    return {
      url: DOWNLOAD_URLS.mac,
      buttonText: "📥 Download for macOS",
      infoText: "macOS Ventura 13.0 or later • Apple Silicon & Intel",
    };
  }
  if (os === "windows") {
    return {
      url: DOWNLOAD_URLS.windows,
      buttonText: "📥 Download for Windows",
      infoText: "Windows 10 or later • 64-bit",
    };
  }
  if (os === "linux") {
    return {
      url: DOWNLOAD_URLS.linux,
      buttonText: "📥 Download for Linux",
      infoText: "Ubuntu 20.04+ or equivalent • x64",
    };
  }

  return {
    buttonText: "📥 Download SpaceKit Desktop",
    infoText: "Available for macOS, Windows & Linux",
  };
}


