import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { createOpenRouterProvider } from "../../src/provider/openrouter.ts";
import type { Message, ToolDefinition } from "../../src/types.ts";
import {
	finishChunk,
	mockErrorResponse,
	mockJsonResponse,
	mockStreamResponse,
	mockTextResponse,
	mockToolCallResponse,
	textChunk,
	toolCallChunk,
} from "../mocks/openrouter.ts";

const TEST_KEY = "sk-or-test-key";
const TEST_MODEL = "openrouter/pony-alpha";
const BASE_URL = "https://openrouter.ai/api/v1";

const originalFetch = globalThis.fetch;

describe("OpenRouter Provider", () => {
	let fetchMock: ReturnType<typeof mock>;

	beforeEach(() => {
		fetchMock = mock();
		globalThis.fetch = fetchMock as unknown as typeof fetch;
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
	});

	const provider = createOpenRouterProvider({
		apiKey: TEST_KEY,
		model: TEST_MODEL,
	});

	const messages: Message[] = [{ role: "user", content: "Hello" }];

	describe("send()", () => {
		test("sends correct request to OpenRouter API", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi there!")));

			await provider.send(messages);

			expect(fetchMock).toHaveBeenCalledTimes(1);
			const [url, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			expect(url).toBe(`${BASE_URL}/chat/completions`);
			expect(opts.method).toBe("POST");
		});

		test("includes correct headers", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await provider.send(messages);

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const headers = opts.headers as Record<string, string>;
			expect(headers.Authorization).toBe(`Bearer ${TEST_KEY}`);
			expect(headers["Content-Type"]).toBe("application/json");
			expect(headers["HTTP-Referer"]).toBeDefined();
		});

		test("sends model and messages in body", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await provider.send(messages);

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.model).toBe(TEST_MODEL);
			expect(body.messages).toEqual(messages);
			expect(body.stream).toBe(false);
		});

		test("includes tools when provided", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			const tools: ToolDefinition[] = [
				{
					type: "function",
					function: {
						name: "read_file",
						description: "Read a file",
						parameters: { type: "object", properties: { path: { type: "string" } }, required: ["path"] },
					},
				},
			];

			await provider.send(messages, tools);

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.tools).toEqual(tools);
		});

		test("parses text response correctly", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hello world!")));

			const response = await provider.send(messages);

			expect(response.id).toBe("chatcmpl-test");
			expect(response.choices[0]?.message.content).toBe("Hello world!");
			expect(response.choices[0]?.finish_reason).toBe("stop");
		});

		test("parses tool call response correctly", async () => {
			const toolResponse = mockToolCallResponse([{ name: "read_file", arguments: { path: "/tmp/test.txt" } }]);
			fetchMock.mockResolvedValueOnce(mockJsonResponse(toolResponse));

			const response = await provider.send(messages);

			const toolCalls = response.choices[0]?.message.tool_calls;
			expect(toolCalls).toHaveLength(1);
			expect(toolCalls?.[0]?.function.name).toBe("read_file");
			expect(JSON.parse(toolCalls?.[0]?.function.arguments ?? "")).toEqual({ path: "/tmp/test.txt" });
		});

		test("throws on non-200 response", async () => {
			fetchMock.mockResolvedValueOnce(mockErrorResponse(401, "Invalid API key"));

			expect(provider.send(messages)).rejects.toThrow();
		});

		test("throws on 429 rate limit", async () => {
			fetchMock.mockResolvedValueOnce(mockErrorResponse(429, "Rate limit exceeded"));

			expect(provider.send(messages)).rejects.toThrow();
		});

		test("send() with overrides merges into request body", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await provider.send(messages, undefined, { temperature: 0.5, max_tokens: 1000 });

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBe(0.5);
			expect(body.max_tokens).toBe(1000);
			expect(body.model).toBe(TEST_MODEL);
		});

		test("per-call model override overrides config model", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await provider.send(messages, undefined, { model: "override-model" });

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.model).toBe("override-model");
		});

		test("per-call temperature override appears in body", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await provider.send(messages, undefined, { temperature: 1.5 });

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBe(1.5);
		});

		test("invalid overrides filtered out by validateOverrides", async () => {
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await provider.send(messages, undefined, { temperature: 5.0, max_tokens: -1 });

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBeUndefined();
			expect(body.max_tokens).toBeUndefined();
		});
	});

	describe("stream()", () => {
		test("sends stream: true in request body", async () => {
			fetchMock.mockResolvedValueOnce(mockStreamResponse([textChunk("Hello"), finishChunk("stop")]));

			const gen = provider.stream(messages);
			// Consume the generator to trigger the fetch
			for await (const _ of gen) {
				// drain
			}

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.stream).toBe(true);
		});

		test("yields text content chunks", async () => {
			fetchMock.mockResolvedValueOnce(
				mockStreamResponse([textChunk("Hello"), textChunk(" world"), finishChunk("stop")]),
			);

			const chunks: string[] = [];
			for await (const chunk of provider.stream(messages)) {
				if (chunk.choices[0]?.delta.content) {
					chunks.push(chunk.choices[0].delta.content);
				}
			}

			expect(chunks).toEqual(["Hello", " world"]);
		});

		test("yields tool call delta chunks", async () => {
			fetchMock.mockResolvedValueOnce(
				mockStreamResponse([
					toolCallChunk(0, { id: "call_0", name: "read_file" }),
					toolCallChunk(0, { arguments: '{"path":' }),
					toolCallChunk(0, { arguments: '"/tmp/test.txt"}' }),
					finishChunk("tool_calls"),
				]),
			);

			const toolNames: string[] = [];
			const argParts: string[] = [];

			for await (const chunk of provider.stream(messages)) {
				const tc = chunk.choices[0]?.delta.tool_calls?.[0];
				if (tc?.function?.name) toolNames.push(tc.function.name);
				if (tc?.function?.arguments) argParts.push(tc.function.arguments);
			}

			expect(toolNames).toEqual(["read_file"]);
			expect(argParts.join("")).toBe('{"path":"/tmp/test.txt"}');
		});

		test("throws on non-200 stream response", async () => {
			fetchMock.mockResolvedValueOnce(mockErrorResponse(500, "Internal server error"));

			const gen = provider.stream(messages);
			expect(async () => {
				for await (const _ of gen) {
					// should throw before yielding
				}
			}).toThrow();
		});

		test("stream() with overrides merges into request body", async () => {
			fetchMock.mockResolvedValueOnce(mockStreamResponse([textChunk("Hi"), finishChunk("stop")]));

			for await (const _ of provider.stream(messages, undefined, { temperature: 0.8 })) {
				// drain
			}

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBe(0.8);
			expect(body.stream).toBe(true);
		});
	});

	describe("requestParams + per-call overrides", () => {
		test("per-call overrides win on conflict with requestParams", async () => {
			const providerWithParams = createOpenRouterProvider({
				apiKey: TEST_KEY,
				model: TEST_MODEL,
				requestParams: { temperature: 0.3, top_p: 0.9 },
			});

			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));

			await providerWithParams.send(messages, undefined, { temperature: 0.8 });

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBe(0.8); // per-call wins
			expect(body.top_p).toBe(0.9); // base preserved
		});
	});

	describe("with()", () => {
		test("returns new provider, original unchanged", async () => {
			const derived = provider.with({ temperature: 0.5 });

			// Original provider should not have temperature
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));
			await provider.send(messages);
			const [, opts1] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body1 = JSON.parse(opts1.body as string);
			expect(body1.temperature).toBeUndefined();

			// Derived provider should have temperature
			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));
			await derived.send(messages);
			const [, opts2] = fetchMock.mock.calls[1] as [string, RequestInit];
			const body2 = JSON.parse(opts2.body as string);
			expect(body2.temperature).toBe(0.5);
		});

		test("with() composes: .with({a}).with({b}) merges correctly", async () => {
			const derived = provider.with({ temperature: 0.5 }).with({ max_tokens: 1000 });

			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));
			await derived.send(messages);

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBe(0.5);
			expect(body.max_tokens).toBe(1000);
		});

		test("with({ model }) changes the model", async () => {
			const derived = provider.with({ model: "different-model" });

			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));
			await derived.send(messages);

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.model).toBe("different-model");
		});

		test("per-call overrides win over with() sticky overrides", async () => {
			const derived = provider.with({ temperature: 0.5 });

			fetchMock.mockResolvedValueOnce(mockJsonResponse(mockTextResponse("Hi")));
			await derived.send(messages, undefined, { temperature: 0.8 });

			const [, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
			const body = JSON.parse(opts.body as string);
			expect(body.temperature).toBe(0.8);
		});
	});
});
