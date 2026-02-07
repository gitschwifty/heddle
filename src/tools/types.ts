import type { TSchema } from "@sinclair/typebox";

export interface HeddleTool {
	name: string;
	description: string;
	parameters: TSchema;
	execute: (params: unknown) => Promise<string>;
}
