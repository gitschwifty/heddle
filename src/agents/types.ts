export interface AgentDefinition {
	name: string;
	description: string;
	model?: string;
	tools?: string[];
	systemPrompt: string;
	source: string;
}
