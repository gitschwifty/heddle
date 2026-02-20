import { type Static, Type } from "@sinclair/typebox";

// ── Config TypeBox Schemas ──────────────────────────────────────────────
// Reusable for headless mode protocol validation (init message config).
// These define the wire format (snake_case) that TOML and JSON both use.

export const ApprovalModeSchema = Type.Union([
	Type.Literal("suggest"),
	Type.Literal("auto-edit"),
	Type.Literal("full-auto"),
	Type.Literal("plan"),
	Type.Literal("yolo"),
]);
export type ApprovalModeSchema = Static<typeof ApprovalModeSchema>;

/** Config fields that flow to the provider (model selection, API params). */
export const ProviderConfigSchema = Type.Object({
	model: Type.Optional(Type.String()),
	weak_model: Type.Optional(Type.String()),
	editor_model: Type.Optional(Type.String()),
	max_tokens: Type.Optional(Type.Number()),
	temperature: Type.Optional(Type.Number()),
	base_url: Type.Optional(Type.String()),
});
export type ProviderConfigSchema = Static<typeof ProviderConfigSchema>;

/** Config fields that flow to session setup (prompts, behavior, limits). */
export const SessionConfigSchema = Type.Object({
	system_prompt: Type.Optional(Type.String()),
	approval_mode: Type.Optional(ApprovalModeSchema),
	instructions: Type.Optional(Type.Array(Type.String())),
	tools: Type.Optional(Type.Array(Type.String())),
	doom_loop_threshold: Type.Optional(Type.Number()),
	budget_limit: Type.Optional(Type.Number()),
});
export type SessionConfigSchema = Static<typeof SessionConfigSchema>;

/** Full heddle config in wire format (snake_case). Used for TOML and headless init. */
export const HeddleConfigSchema = Type.Object({
	api_key: Type.Optional(Type.String()),
	model: Type.Optional(Type.String()),
	weak_model: Type.Optional(Type.String()),
	editor_model: Type.Optional(Type.String()),
	max_tokens: Type.Optional(Type.Number()),
	temperature: Type.Optional(Type.Number()),
	base_url: Type.Optional(Type.String()),
	system_prompt: Type.Optional(Type.String()),
	approval_mode: Type.Optional(ApprovalModeSchema),
	instructions: Type.Optional(Type.Array(Type.String())),
	tools: Type.Optional(Type.Array(Type.String())),
	doom_loop_threshold: Type.Optional(Type.Number()),
	budget_limit: Type.Optional(Type.Number()),
});
export type HeddleConfigSchema = Static<typeof HeddleConfigSchema>;
