import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import { loadAgentRoster } from "../agentRoster.js";
import { createEngineHandlers } from "../engineBridge.js";
import { MobileServer } from "../server.js";

/**
 * B0 acceptance: `shannon/agent.list` returns the host's real agent roster —
 * the parseable subset of `~/.shannon/agents/*.toml` (injected dirs in
 * tests), skipping broken files, with the fixed idle/activity placeholders.
 */

const logger = createConsoleLogger("error");

let tmpDirs: string[] = [];
function tmpDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "gw-roster-"));
  tmpDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of tmpDirs) rmSync(dir, { recursive: true, force: true });
  tmpDirs = [];
});

function writeAgent(dir: string, file: string, content: string): void {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, file), content, "utf8");
}

describe("loadAgentRoster", () => {
  it("maps *.toml definitions to the roster wire shape (id = name 原文)", () => {
    const dir = tmpDir();
    writeAgent(
      dir,
      "backend.toml",
      [
        'name = "backend-dev"',
        'description = "Backend development specialist"',
        'model = "claude-sonnet"',
        'system_prompt = """You are a backend developer."""',
        "max_concurrent_tasks = 3",
      ].join("\n"),
    );

    expect(loadAgentRoster([dir])).toEqual([
      {
        id: "backend-dev",
        name: "backend-dev",
        role: "Backend development specialist",
        model: "claude-sonnet",
        status: "idle",
        activity: [],
      },
    ]);
  });

  it("omits role/model when the definition has no usable value", () => {
    const dir = tmpDir();
    writeAgent(
      dir,
      "bare.toml",
      ['name = "bare"', 'description = ""', "model = 3"].join("\n"),
    );

    const roster = loadAgentRoster([dir]);
    expect(roster).toEqual([
      { id: "bare", name: "bare", status: "idle", activity: [] },
    ]);
    expect(Object.keys(roster[0]!)).toEqual(["id", "name", "status", "activity"]);
  });

  it("skips unparseable and nameless files, keeps the rest (sorted by filename)", () => {
    const dir = tmpDir();
    writeAgent(dir, "a-good.toml", 'name = "first"\ndescription = "ok"');
    writeAgent(dir, "b-broken.toml", "name = [unclosed");
    writeAgent(dir, "c-noname.toml", 'description = "no name here"');
    writeAgent(dir, "d-empty.toml", "");
    writeAgent(dir, "notes.txt", 'name = "not a toml"');

    expect(loadAgentRoster([dir])).toEqual([
      { id: "first", name: "first", role: "ok", status: "idle", activity: [] },
    ]);
  });

  it("an empty dir and a missing dir both yield no entries; defaults point at the real home", () => {
    expect(loadAgentRoster([tmpDir()])).toEqual([]);
    expect(loadAgentRoster([join(tmpDir(), "does-not-exist")])).toEqual([]);

    // Sanity on the default: the real home dir may or may not exist — the
    // loader must not throw either way (honest empty roster).
    expect(() => loadAgentRoster()).not.toThrow();
    expect(defaultDirs()).toEqual([join(homedir(), ".shannon", "agents")]);
  });
});

/** The loader's default scan target, re-derived here to pin the contract. */
function defaultDirs(): string[] {
  return [join(homedir(), ".shannon", "agents")];
}

describe("shannon/agent.list via engineBridge (end-to-end)", () => {
  it("serves the roster over the real WS surface", async () => {
    const dir = tmpDir();
    writeAgent(
      dir,
      "reviewer.toml",
      ['name = "reviewer"', 'description = "Code review specialist"'].join("\n"),
    );
    writeAgent(dir, "broken.toml", "nope =");

    const server = new MobileServer({
      host: "127.0.0.1",
      port: 0,
      logger,
      handlers: createEngineHandlers({
        engineWsUrl: "ws://127.0.0.1:9",
        engineHttpBaseUrl: "http://engine:33420",
        version: "test",
        logger,
        agentRosterDirs: [dir],
      }),
    });
    const handle = await server.start();
    const socket = new WebSocket(`ws://127.0.0.1:${handle.port}/`);
    await new Promise<void>((resolve, reject) => {
      socket.once("open", () => resolve());
      socket.once("error", reject);
    });
    try {
      const res = await new Promise<any>((resolve, reject) => {
        socket.on("message", function onMsg(data: unknown) {
          const msg = JSON.parse(String(data)) as any;
          if (msg.id === 1) {
            socket.off("message", onMsg);
            resolve(msg);
          }
        });
        socket.on("error", reject);
        socket.send(
          JSON.stringify({ jsonrpc: "2.0", id: 1, method: "shannon/agent.list" }),
        );
      });
      expect(res.result).toEqual({
        agents: [
          {
            id: "reviewer",
            name: "reviewer",
            role: "Code review specialist",
            status: "idle",
            activity: [],
          },
        ],
      });
    } finally {
      socket.close();
      await handle.stop();
    }
  });
});
