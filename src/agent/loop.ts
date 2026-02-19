import type { Provider } from "../provider/types.ts";
import type { ToolRegistry } from "../tools/registry.ts";
import type { AssistantMessage, Message, ToolMessage } from "../types.ts";
import type { AgentEvent } from "./types.ts";

export interface AgentLoopOptions {
  maxIterations?: number;
}

const DEFAULT_MAX_ITERATIONS = 20;

/**
 * Core agent loop: send messages → if tool_calls, execute tools → append results → repeat.
 * Mutates the passed-in messages array directly (appends assistant + tool messages).
 * Terminates when the assistant response has no tool_calls (text-only) or max iterations reached.
 */
export async function* runAgentLoop(
  provider: Provider,
  registry: ToolRegistry,
  messages: Message[],
  options?: AgentLoopOptions,
): AsyncGenerator<AgentEvent> {
  const maxIterations = options?.maxIterations ?? DEFAULT_MAX_ITERATIONS;
  const tools = registry.definitions();

  for (let iteration = 0; iteration < maxIterations; iteration++) {
    const response = await provider.send(
      messages,
      tools.length > 0 ? tools : undefined,
    );
    const choice = response.choices[0];
    if (!choice) {
      yield { type: "error", error: new Error("No choice in response") };
      return;
    }

    const assistantMsg: AssistantMessage = {
      role: "assistant",
      content: choice.message.content,
      ...(choice.message.tool_calls?.length
        ? { tool_calls: choice.message.tool_calls }
        : {}),
    };

    yield { type: "assistant_message", message: assistantMsg };
    messages.push(assistantMsg);

    const toolCalls = choice.message.tool_calls;
    if (!toolCalls?.length) {
      // No tool calls — the loop is done
      return;
    }

    // Execute each tool call and collect results
    const toolMessages: ToolMessage[] = [];
    for (const call of toolCalls) {
      yield { type: "tool_start", name: call.function.name, call };

      const result = await registry.execute(
        call.function.name,
        call.function.arguments,
      );

      yield { type: "tool_end", name: call.function.name, result, call };

      toolMessages.push({
        role: "tool",
        tool_call_id: call.id,
        content: result,
      });
    }

    // Append all tool results to messages
    messages.push(...toolMessages);
  }

  // If we get here, we've hit the iteration limit
  yield {
    type: "error",
    error: new Error(
      `Max iterations (${maxIterations}) reached — possible infinite loop`,
    ),
  };
}
