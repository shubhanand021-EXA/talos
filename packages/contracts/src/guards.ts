import type { z } from "zod";

/** Generic validator result without throwing. */
export type ValidationSuccess<T> = { ok: true; value: T };
export type ValidationFailure = { ok: false; issues: string };
export type ValidationResult<T> = ValidationSuccess<T> | ValidationFailure;

/**
 * Validate `value` against `schema`, returning a discriminated result
 * instead of throwing. Useful at IPC and Tauri bridge boundaries.
 */
export function validateWith<T>(
  schema: z.ZodType<T>,
  value: unknown,
): ValidationResult<T> {
  const result = schema.safeParse(value);
  if (result.success) {
    return { ok: true, value: result.data };
  }
  return { ok: false, issues: result.error.message };
}

/** Type guard for enum-like zod schemas. */
export function isValidEnum<T extends string>(
  schema: z.ZodType<T>,
  value: unknown,
): value is T {
  return schema.safeParse(value).success;
}
