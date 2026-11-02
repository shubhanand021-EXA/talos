import { z } from "zod";

/** Update status returned by `check_for_updates`. */
export const UpdateInfoSchema = z
  .object({
    available: z.boolean(),
    currentVersion: z.string().min(1).max(64),
    latestVersion: z.string().min(1).max(64).nullish(),
    notes: z.string().max(8192).nullish(),
    pubDate: z.string().max(64).nullish(),
    downloadUrl: z.string().min(1).max(1024).nullish(),
  })
  .strict();
export type UpdateInfo = z.infer<typeof UpdateInfoSchema>;
