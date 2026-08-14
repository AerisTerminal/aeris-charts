/** Stable machine-readable categories for public API failures. */
export type nucleuscharts_error_code =
  | "disposed"
  | "invalid_handle"
  | "stale_handle"
  | "invalid_data"
  | "invalid_options"
  | "unsupported_operation"
  | "serialization_error"
  | "persistence_version_error"
  | "extension_error"
  | "renderer_platform_error"
  | "resource_limit";

/** Public error thrown for predictable chart, handle, validation, persistence, and platform failures. */
export class nucleuscharts_error extends Error {
  override readonly name = "NucleusChartsError";

  constructor(
    readonly code: nucleuscharts_error_code,
    message: string,
  ) {
    super(`nucleuscharts: ${message}`);
  }
}
