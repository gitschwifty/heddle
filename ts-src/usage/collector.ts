export interface SessionMetrics {
	messageCount: { user: number; assistant: number };
	toolCalls: Record<string, number>;
	errors: { tool: number; provider: number };
	tokens: { input: number; output: number };
	turns: number;
}

export class MetricsCollector {
	private _userMessages = 0;
	private _assistantMessages = 0;
	private _toolCalls: Record<string, number> = {};
	private _toolErrors = 0;
	private _providerErrors = 0;
	private _inputTokens = 0;
	private _outputTokens = 0;
	private _turns = 0;

	onAssistantMessage(): void {
		this._assistantMessages++;
	}

	onUserMessage(): void {
		this._userMessages++;
		this._turns++;
	}

	onToolCall(name: string): void {
		this._toolCalls[name] = (this._toolCalls[name] ?? 0) + 1;
	}

	onError(source: "tool" | "provider"): void {
		if (source === "tool") {
			this._toolErrors++;
		} else {
			this._providerErrors++;
		}
	}

	onUsage(usage: { prompt_tokens: number; completion_tokens: number }): void {
		this._inputTokens += usage.prompt_tokens;
		this._outputTokens += usage.completion_tokens;
	}

	get metrics(): SessionMetrics {
		return {
			messageCount: { user: this._userMessages, assistant: this._assistantMessages },
			toolCalls: { ...this._toolCalls },
			errors: { tool: this._toolErrors, provider: this._providerErrors },
			tokens: { input: this._inputTokens, output: this._outputTokens },
			turns: this._turns,
		};
	}
}
