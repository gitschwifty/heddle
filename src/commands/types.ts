import type * as readline from "node:readline";
import type { AgentDefinition } from "../agents/types.ts";
import type { DiscoveryResult } from "../config/discovery.ts";
import type { HeddleConfig } from "../config/loader.ts";
import type { CostTracker } from "../cost/tracker.ts";
import type { Provider } from "../provider/types.ts";
import type { ToolRegistry } from "../tools/registry.ts";
import type { Message } from "../types.ts";

export interface SlashCommand {
	name: string;
	description: string;
	execute: (args: string, ctx: CommandContext) => Promise<void>;
}

export interface CommandContext {
	config: HeddleConfig;
	messages: Message[];
	registry: ToolRegistry;
	costTracker: CostTracker;
	sessionFile: string;
	sessionId: string;
	provider: Provider;
	weakProvider?: Provider;
	editorProvider?: Provider;
	discovery?: DiscoveryResult;
	agentDefinitions: Map<string, AgentDefinition>;
	rl: readline.Interface;
	setModel: (model: string) => void;
}
