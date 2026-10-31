import { z } from "zod";
import { IsoDateTimeSchema, TalosIdSchema, TaskIdSchema } from "./identity.js";

export const ComputerActionSchema = z.enum([
  "move_mouse",
  "click",
  "double_click",
  "drag",
  "type_text",
  "hotkey",
  "scroll",
  "capture_screen",
  "locate_element",
]);
export type ComputerAction = z.infer<typeof ComputerActionSchema>;

const CoordinateSchema = z.number().int().min(0).max(10000);

const BoundingBoxSchema = z
  .object({
    x: CoordinateSchema,
    y: CoordinateSchema,
    width: z.number().int().min(1).max(10000),
    height: z.number().int().min(1).max(10000),
  })
  .strict();
export type ComputerBoundingBox = z.infer<typeof BoundingBoxSchema>;

/** Validated computer-use target: coordinates, bounding box, app/window hint. */
export const ComputerTargetSchema = z
  .object({
    x: CoordinateSchema.optional(),
    y: CoordinateSchema.optional(),
    width: z.number().int().min(1).max(10000).optional(),
    height: z.number().int().min(1).max(10000).optional(),
    appHint: z.string().min(1).max(256).optional(),
    windowHint: z.string().min(1).max(256).optional(),
    confidence: z.number().min(0).max(1).optional(),
  })
  .strict();
export type ComputerTarget = z.infer<typeof ComputerTargetSchema>;

/** One validated computer action invocation. */
export const ComputerActionRequestSchema = z
  .object({
    actionId: TalosIdSchema,
    taskId: TaskIdSchema,
    action: ComputerActionSchema,
    target: ComputerTargetSchema,
    text: z.string().min(1).max(4096).optional(),
    keys: z.string().min(1).max(64).optional(),
    direction: z.enum(["up", "down", "left", "right"]).optional(),
    pixels: z.number().int().min(1).max(10000).optional(),
    highRisk: z.boolean().optional(),
    highRiskKind: z
      .enum([
        "form_submit",
        "purchase",
        "settings_change",
        "file_upload",
        "file_download",
        "destructive",
        "other",
      ])
      .optional(),
    dryRun: z.boolean().optional(),
  })
  .strict()
  .refine(
    (request) =>
      request.highRiskKind === undefined || request.highRisk === true,
    "highRiskKind requires highRisk: true",
  )
  .refine(
    (request) => request.action !== "type_text" || request.text !== undefined,
    "type_text requires text",
  )
  .refine(
    (request) => request.action !== "hotkey" || request.keys !== undefined,
    "hotkey requires keys",
  );
export type ComputerActionRequest = z.infer<typeof ComputerActionRequestSchema>;

/** Auditable computer receipt: hashes, bounding box, confidence, gating refs. */
export const ComputerReceiptSchema = z
  .object({
    id: TalosIdSchema,
    actionId: TalosIdSchema,
    taskId: TaskIdSchema,
    action: ComputerActionSchema,
    status: z.enum(["success", "failed", "denied", "disabled"]),
    dryRun: z.boolean(),
    screenHash: z.string().min(1).max(256).optional(),
    boundingBox: BoundingBoxSchema.optional(),
    confidence: z.number().min(0).max(1).optional(),
    targetUrl: z.string().max(2048).optional(),
    policyDecision: z.string().min(1).max(64),
    policyReason: z.string().min(1).max(2048),
    evidenceId: z.string().min(1).max(128).optional(),
    compensationPlanId: z.string().min(1).max(128).optional(),
    highRisk: z.boolean().optional(),
    warnings: z.array(z.string().max(512)).max(16).optional(),
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
export type ComputerReceipt = z.infer<typeof ComputerReceiptSchema>;
