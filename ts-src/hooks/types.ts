import { type Static, Type } from "@sinclair/typebox";

// ── Hook Event ─────────────────────────────────────────────────────────

export const HookEventSchema = Type.Union([
	Type.Literal("session_start"),
	Type.Literal("session_end"),
	Type.Literal("pre_prompt"),
	Type.Literal("pre_tool"),
	Type.Literal("post_tool"),
	Type.Literal("post_turn"),
	Type.Literal("error"),
]);
export type HookEvent = Static<typeof HookEventSchema>;

// ── Hook Mode ──────────────────────────────────────────────────────────

export const HookModeSchema = Type.Union([Type.Literal("interactive"), Type.Literal("headless"), Type.Literal("both")]);
export type HookMode = Static<typeof HookModeSchema>;

// ── Matchers ───────────────────────────────────────────────────────────

export const HookMatchersSchema = Type.Object({
	tool: Type.Optional(Type.Union([Type.String(), Type.Array(Type.String())])),
	match_path: Type.Optional(Type.String()),
	match_args: Type.Optional(Type.String()),
	match_input: Type.Optional(Type.String()),
});
export type HookMatchers = Static<typeof HookMatchersSchema>;

// ── Hook Definition ────────────────────────────────────────────────────

export const HookDefinitionSchema = Type.Object({
	command: Type.String(),
	timeout: Type.Optional(Type.Number({ default: 10000 })),
	mode: Type.Optional(
		Type.Union([Type.Literal("interactive"), Type.Literal("headless"), Type.Literal("both")], {
			default: "both",
		}),
	),
	async: Type.Optional(Type.Boolean({ default: false })),
	matchers: Type.Optional(HookMatchersSchema),
});
export type HookDefinition = Static<typeof HookDefinitionSchema>;

// ── Hooks Config ───────────────────────────────────────────────────────

export const HooksConfigSchema = Type.Object(
	{
		session_start: Type.Optional(Type.Array(HookDefinitionSchema)),
		session_end: Type.Optional(Type.Array(HookDefinitionSchema)),
		pre_prompt: Type.Optional(Type.Array(HookDefinitionSchema)),
		pre_tool: Type.Optional(Type.Array(HookDefinitionSchema)),
		post_tool: Type.Optional(Type.Array(HookDefinitionSchema)),
		post_turn: Type.Optional(Type.Array(HookDefinitionSchema)),
		error: Type.Optional(Type.Array(HookDefinitionSchema)),
	},
	{ additionalProperties: false },
);
export type HooksConfig = Static<typeof HooksConfigSchema>;

/** Runtime hook definition with defaults applied (used by runner/matcher). */
export interface ResolvedHookDefinition {
	command: string;
	timeout: number;
	mode: "interactive" | "headless" | "both";
	async: boolean;
	matchers?: HookMatchers;
}

/** Runtime hooks config with resolved definitions. */
export type ResolvedHooksConfig = {
	[K in HookEvent]?: ResolvedHookDefinition[];
};

// ── Context & Result Interfaces ────────────────────────────────────────

export interface HookContext {
	sessionId: string;
	project: string;
	model: string;
	event: string;
	toolName?: string;
	toolArgs?: string;
	toolResult?: string;
	userInput?: string;
}

export interface HookResult {
	blocked: boolean;
	reason?: string;
	feedback?: string;
	timedOut: boolean;
}
