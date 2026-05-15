import { Value } from "@sinclair/typebox/value";
import { IpcRequestSchema, IpcResponseSchema } from "./types.ts";

export function validateIpcMessage(msg: unknown): boolean {
	return Value.Check(IpcRequestSchema, msg) || Value.Check(IpcResponseSchema, msg);
}
