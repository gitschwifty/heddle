import { type Static, Type } from "@sinclair/typebox";

export const InitConfigSchema = Type.Object({
	model: Type.String(),
	system_prompt: Type.String(),
	tools: Type.Array(Type.String()),
	max_iterations: Type.Optional(Type.Number()),
});

export const IpcRequestSchema = Type.Union([
	Type.Object({
		type: Type.Literal("init"),
		id: Type.String(),
		protocol_version: Type.Optional(Type.String()),
		config: InitConfigSchema,
	}),
	Type.Object({ type: Type.Literal("send"), id: Type.String(), message: Type.String() }),
	Type.Object({ type: Type.Literal("status"), id: Type.String() }),
	Type.Object({ type: Type.Literal("shutdown"), id: Type.String() }),
	Type.Object({ type: Type.Literal("cancel"), id: Type.String(), target_id: Type.String() }),
]);

export const WorkerEventSchema = Type.Union([
	Type.Object({ event: Type.Literal("content_delta"), text: Type.String() }),
	Type.Object({ event: Type.Literal("tool_start"), name: Type.String(), args: Type.Unknown() }),
	Type.Object({ event: Type.Literal("tool_end"), name: Type.String(), result_preview: Type.String() }),
	Type.Object({
		event: Type.Literal("usage"),
		prompt_tokens: Type.Number(),
		completion_tokens: Type.Number(),
		total_tokens: Type.Number(),
	}),
	Type.Object({ event: Type.Literal("error"), error: Type.String(), code: Type.Optional(Type.String()) }),
]);

export const IpcResponseSchema = Type.Union([
	Type.Object({
		type: Type.Literal("init_ok"),
		id: Type.String(),
		session_id: Type.String(),
		protocol_version: Type.String(),
		error: Type.Optional(Type.String()),
	}),
	Type.Object({ type: Type.Literal("event"), event: WorkerEventSchema }),
	Type.Object({
		type: Type.Literal("result"),
		id: Type.String(),
		status: Type.String(),
		response: Type.Optional(Type.String()),
		tool_calls_made: Type.Array(Type.Object({ name: Type.String(), args: Type.Unknown() })),
		usage: Type.Optional(
			Type.Object({ prompt_tokens: Type.Number(), completion_tokens: Type.Number(), total_tokens: Type.Number() }),
		),
		iterations: Type.Number(),
		error: Type.Optional(Type.String()),
	}),
	Type.Object({
		type: Type.Literal("status_ok"),
		id: Type.String(),
		model: Type.String(),
		messages_count: Type.Number(),
		session_id: Type.String(),
		active: Type.Boolean(),
	}),
	Type.Object({ type: Type.Literal("shutdown_ok"), id: Type.String() }),
]);

export type InitConfig = Static<typeof InitConfigSchema>;
export type IpcRequest = Static<typeof IpcRequestSchema>;
export type IpcResponse = Static<typeof IpcResponseSchema>;
export type WorkerEvent = Static<typeof WorkerEventSchema>;
