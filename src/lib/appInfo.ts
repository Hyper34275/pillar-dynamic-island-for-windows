// Product constants. The display name and identifier are defined once per layer
// (here, tauri.conf.json, paths.rs); changing the product name means editing those three.

export const APP_NAME = "Yuval";
export const APP_IDENTIFIER = "com.companyisland.app";
export const APP_VERSION: string = __APP_VERSION__;

/** Stable error codes shown in the UI; the full catalogue lives in docs/ENTERPRISE_DESIGN.md. */
export const ERROR_CODES = {
  uiRender: "APP-001",
} as const;
