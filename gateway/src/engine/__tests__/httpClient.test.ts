import { describe, expect, it, vi } from "vitest";

import { ENGINE_HTTP_TIMEOUT_MS, respondToApproval } from "../httpClient.js";

function ok(): Response {
  return new Response("{}", { status: 200 });
}

describe("respondToApproval", () => {
  it("posts allow mapped to allow_once with the right body", async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => ok());
    await respondToApproval({
      engineBaseUrl: "http://127.0.0.1:33420",
      requestId: "req-1",
      choice: "allow",
      fetchImpl,
    });

    expect(fetchImpl).toHaveBeenCalledTimes(1);
    const call = fetchImpl.mock.calls[0]!;
    expect(call[0]).toBe("http://127.0.0.1:33420/api/approval/respond");
    const init = call[1]!;
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({
      request_id: "req-1",
      choice: "allow_once",
    });
  });

  it("posts deny unchanged", async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => ok());
    await respondToApproval({
      engineBaseUrl: "http://e",
      requestId: "r2",
      choice: "deny",
      fetchImpl,
    });
    const init = fetchImpl.mock.calls[0]![1]!;
    expect(JSON.parse(init.body as string)).toEqual({
      request_id: "r2",
      choice: "deny",
    });
  });

  it("strips trailing slashes from the base url", async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => ok());
    await respondToApproval({
      engineBaseUrl: "http://e///",
      requestId: "r",
      choice: "allow",
      fetchImpl,
    });
    expect(fetchImpl.mock.calls[0]![0]).toBe("http://e/api/approval/respond");
  });

  it("throws on non-2xx", async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => new Response("nope", { status: 404 }));
    await expect(
      respondToApproval({
        engineBaseUrl: "http://e",
        requestId: "r",
        choice: "deny",
        fetchImpl,
      }),
    ).rejects.toThrow(/HTTP 404/);
  });

  // review §P2-23: the approval POST must carry an abort signal so a wedged
  // engine can't hang the adapter's approval path forever.
  it("sends an engine-budget abort signal with the request", async () => {
    const fetchImpl = vi.fn<typeof fetch>(async () => ok());
    await respondToApproval({
      engineBaseUrl: "http://e",
      requestId: "r",
      choice: "allow",
      fetchImpl,
    });
    const init = fetchImpl.mock.calls[0]![1]!;
    const signal = init.signal as AbortSignal;
    expect(signal).toBeInstanceOf(AbortSignal);
    expect(signal.aborted).toBe(false);
  });

  it("aborts with a TimeoutError when the engine never responds", async () => {
    // A fetchImpl that mirrors the platform's behavior: rejects only when the
    // caller-supplied signal aborts, never resolves.
    const fetchImpl = vi.fn<typeof fetch>((_url, init) => {
      return new Promise<Response>((_, reject) => {
        init?.signal?.addEventListener("abort", () => {
          reject(new DOMException("The operation was aborted due to timeout", "TimeoutError"));
        });
      });
    });
    await expect(
      respondToApproval({
        engineBaseUrl: "http://e",
        requestId: "r",
        choice: "allow",
        fetchImpl,
        timeoutMs: 30,
      }),
    ).rejects.toThrow(/timeout/i);
    // default budget constant is sane
    expect(ENGINE_HTTP_TIMEOUT_MS).toBe(60_000);
  });
});
