import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { createOpenRouterProvider } from "../../src/provider/openrouter.ts";

const originalFetch = globalThis.fetch;

describe("OpenRouter Provider (negative)", () => {
	let fetchMock: ReturnType<typeof mock>;

	beforeEach(() => {
		fetchMock = mock();
		globalThis.fetch = fetchMock as unknown as typeof fetch;
	});

	afterEach(() => {
		globalThis.fetch = originalFetch;
	});

	const provider = createOpenRouterProvider({
		apiKey: "sk-test",
		model: "test-model",
	});

	const messages = [{ role: "user" as const, content: "Hi" }];

	describe("send() error handling", () => {
		test("throws on network failure", async () => {
			fetchMock.mockRejectedValueOnce(new TypeError("Failed to fetch"));

			expect(provider.send(messages)).rejects.toThrow("Failed to fetch");
		});

		test("includes status code in error message", async () => {
			fetchMock.mockResolvedValueOnce(
				new Response(JSON.stringify({ error: { message: "Unauthorized" } }), { status: 401 }),
			);

			expect(provider.send(messages)).rejects.toThrow("401");
		});

		test("includes error body in error message", async () => {
			fetchMock.mockResolvedValueOnce(
				new Response("Internal server error", { status: 500 }),
			);

			expect(provider.send(messages)).rejects.toThrow("Internal server error");
		});

		test("throws on 403 forbidden", async () => {
			fetchMock.mockResolvedValueOnce(
				new Response(JSON.stringify({ error: { message: "Forbidden" } }), { status: 403 }),
			);

			expect(provider.send(messages)).rejects.toThrow();
		});
	});

	describe("stream() error handling", () => {
		test("throws on network failure", async () => {
			fetchMock.mockRejectedValueOnce(new TypeError("Network error"));

			const gen = provider.stream(messages);
			expect(async () => {
				for await (const _ of gen) {
					/* drain */
				}
			}).toThrow("Network error");
		});

		test("throws when response body is null", async () => {
			// Construct a 200 response with no body
			fetchMock.mockResolvedValueOnce(
				new Response(null, { status: 200, headers: { "Content-Type": "text/event-stream" } }),
			);

			const gen = provider.stream(messages);
			expect(async () => {
				for await (const _ of gen) {
					/* drain */
				}
			}).toThrow("No response body");
		});

		test("throws on malformed SSE JSON", async () => {
			const encoder = new TextEncoder();
			const stream = new ReadableStream({
				start(controller) {
					controller.enqueue(encoder.encode("data: {invalid json}\n\n"));
					controller.close();
				},
			});
			fetchMock.mockResolvedValueOnce(
				new Response(stream, { status: 200, headers: { "Content-Type": "text/event-stream" } }),
			);

			const gen = provider.stream(messages);
			expect(async () => {
				for await (const _ of gen) {
					/* drain */
				}
			}).toThrow();
		});

		test("handles SSE stream with only [DONE] (no content chunks)", async () => {
			const encoder = new TextEncoder();
			const stream = new ReadableStream({
				start(controller) {
					controller.enqueue(encoder.encode("data: [DONE]\n\n"));
					controller.close();
				},
			});
			fetchMock.mockResolvedValueOnce(
				new Response(stream, { status: 200, headers: { "Content-Type": "text/event-stream" } }),
			);

			const chunks: unknown[] = [];
			for await (const chunk of provider.stream(messages)) {
				chunks.push(chunk);
			}
			expect(chunks).toHaveLength(0);
		});

		test("ignores non-data SSE lines (comments, empty lines)", async () => {
			const encoder = new TextEncoder();
			const stream = new ReadableStream({
				start(controller) {
					controller.enqueue(encoder.encode(": this is a comment\n\n"));
					controller.enqueue(encoder.encode("\n"));
					controller.enqueue(
						encoder.encode(
							`data: ${JSON.stringify({ id: "test", choices: [{ index: 0, delta: { content: "hi" }, finish_reason: null }] })}\n\n`,
						),
					);
					controller.enqueue(encoder.encode("data: [DONE]\n\n"));
					controller.close();
				},
			});
			fetchMock.mockResolvedValueOnce(
				new Response(stream, { status: 200, headers: { "Content-Type": "text/event-stream" } }),
			);

			const chunks: unknown[] = [];
			for await (const chunk of provider.stream(messages)) {
				chunks.push(chunk);
			}
			expect(chunks).toHaveLength(1);
		});
	});
});
