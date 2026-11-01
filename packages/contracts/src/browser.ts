import { z } from "zod";
import { IsoDateTimeSchema, TalosIdSchema, TaskIdSchema } from "./identity.js";
import { ExecutionFidelitySchema } from "./fidelity.js";

export const BrowserActionSchema = z.enum([
  "navigate",
  "click",
  "type",
  "scroll",
  "select",
  "extract_text",
  "screenshot",
  "snapshot_dom",
]);
export type BrowserAction = z.infer<typeof BrowserActionSchema>;

export const BrowserDirectionSchema = z.enum(["up", "down", "top", "bottom"]);
export type BrowserDirection = z.infer<typeof BrowserDirectionSchema>;

export const HighRiskKindSchema = z.enum([
  "form_submit",
  "purchase",
  "settings_change",
  "file_upload",
  "file_download",
  "destructive",
  "other",
]);
export type HighRiskKind = z.infer<typeof HighRiskKindSchema>;

const HttpUrlSchema = z
  .string()
  .min(1)
  .max(2048)
  .refine(
    (url) => url.startsWith("https://") || url.startsWith("http://"),
    "url must use http(s)",
  )
  .refine((url) => !url.includes("@"), "url must not embed credentials");

const SelectorSchema = z.string().min(1).max(512);

/** Validated browser target: URL + optional scope, frame, and timeout. */
export const BrowserTargetSchema = z
  .object({
    url: HttpUrlSchema.optional(),
    selector: SelectorSchema.optional(),
    frame: z.string().min(1).max(128).optional(),
    timeoutMs: z.number().int().min(100).max(60000).optional(),
  })
  .strict();
export type BrowserTarget = z.infer<typeof BrowserTargetSchema>;

/** One validated browser action invocation. */
export const BrowserActionRequestSchema = z
  .object({
    actionId: TalosIdSchema,
    taskId: TaskIdSchema,
    action: BrowserActionSchema,
    target: BrowserTargetSchema,
    text: z.string().min(1).max(4096).optional(),
    value: z.string().min(1).max(1024).optional(),
    direction: BrowserDirectionSchema.optional(),
    pixels: z.number().int().min(1).max(10000).optional(),
    maxChars: z.number().int().min(1).max(20000).optional(),
    fullPage: z.boolean().optional(),
    submit: z.boolean().optional(),
    highRisk: z.boolean().optional(),
    highRiskKind: HighRiskKindSchema.optional(),
    dryRun: z.boolean().optional(),
    approved: z.boolean().optional(),
    thumbnail: z.boolean().optional(),
    allowedOrigins: z.array(z.string().min(1).max(2048)).max(16).optional(),
  })
  .strict()
  .refine(
    (request) =>
      request.highRiskKind === undefined || request.highRisk === true,
    "highRiskKind requires highRisk: true",
  );
export type BrowserActionRequest = z.infer<typeof BrowserActionRequestSchema>;

/** Selector resolution report (DOM-first safety model). */
export const SelectorReportSchema = z
  .object({
    selector: SelectorSchema,
    found: z.boolean(),
    visible: z.boolean(),
    interactable: z.boolean(),
    matchCount: z.number().int().nonnegative(),
    domHash: z.string().min(1).max(256),
  })
  .strict();
export type SelectorReport = z.infer<typeof SelectorReportSchema>;

/** Auditable browser receipt: hashes, optional text, and gating references. */
export const BrowserReceiptSchema = z
  .object({
    id: TalosIdSchema,
    actionId: TalosIdSchema,
    taskId: TaskIdSchema,
    action: BrowserActionSchema,
    status: z.enum(["success", "failed", "denied", "disabled"]),
    dryRun: z.boolean(),
    domHash: z.string().min(1).max(256).optional(),
    screenshotHash: z.string().min(1).max(256).optional(),
    extractedText: z.string().max(20000).optional(),
    extractedTextHash: z.string().min(1).max(256).optional(),
    resolved: SelectorReportSchema.optional(),
    targetUrl: z.string().max(2048).optional(),
    policyDecision: z.string().min(1).max(64),
    policyReason: z.string().min(1).max(2048),
    evidenceId: z.string().min(1).max(128).optional(),
    compensationPlanId: z.string().min(1).max(128).optional(),
    executionFidelity: ExecutionFidelitySchema,
    highRisk: z.boolean().optional(),
    highRiskKind: HighRiskKindSchema.optional(),
    error: z
      .object({
        code: z.string().min(1).max(64),
        message: z.string().min(1).max(2048),
      })
      .strict()
      .optional(),
    executedAt: IsoDateTimeSchema,
  })
  .strict();
export type BrowserReceipt = z.infer<typeof BrowserReceiptSchema>;
