import { z } from "zod";

export const ProviderPaseoToolsPolicySchema = z.object({
  enabled: z.boolean().optional(),
  disabledTools: z.array(z.string()).optional(),
});

const ProviderProfileThinkingOptionSchema = z.object({
  id: z.string(),
  label: z.string(),
  description: z.string().optional(),
  isDefault: z.boolean().optional(),
});

export const ProviderProfileModelSchema = z.object({
  id: z.string().min(1),
  label: z.string().min(1),
  description: z.string().optional(),
  isDefault: z.boolean().optional(),
  thinkingOptions: z.array(ProviderProfileThinkingOptionSchema).optional(),
});

export type ProviderProfileModel = z.infer<typeof ProviderProfileModelSchema>;
