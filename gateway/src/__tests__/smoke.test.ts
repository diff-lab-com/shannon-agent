import { describe, it, expect } from "vitest";

import { GATEWAY_VERSION, resolveRunConfigPath } from "../index.js";
import { buildUnit } from "../service/units.js";
import { configPathForProfile } from "../service/service.js";

describe("scaffold smoke", () => {
  it("exposes a version string", () => {
    expect(typeof GATEWAY_VERSION).toBe("string");
    expect(GATEWAY_VERSION.length).toBeGreaterThan(0);
  });
});

// ── review F43: `run --profile <p>` must resolve the profile's config ────

describe("resolveRunConfigPath (review F43)", () => {
  it("maps --profile to the profile config path via configPathForProfile", () => {
    const r = resolveRunConfigPath(["--profile", "work"]);
    expect(r.configPath).toBe(configPathForProfile("work"));
    expect(r.warning).toBeUndefined();
  });

  it("prefers --config and warns when both flags are given", () => {
    const r = resolveRunConfigPath(["--config", "/tmp/my-config.json", "--profile", "work"]);
    expect(r.configPath).toBe("/tmp/my-config.json");
    expect(r.warning).toMatch(/--config.*wins|--config.*profile/s);
  });

  it("returns nothing when neither flag is given (loader applies env/default)", () => {
    expect(resolveRunConfigPath([]).configPath).toBeUndefined();
    expect(resolveRunConfigPath(["run"]).configPath).toBeUndefined();
  });

  // Round-trip the deployment path: `install --profile work` writes a unit
  // whose args must land back on the profile config when run parses them.
  it("round-trips install → unit args → run config resolution", () => {
    // systemd: ExecStart=<bin> run --profile work
    const linux = buildUnit("linux", "/bin/shannon-gateway", "work");
    const execStart = linux.contents
      .split("\n")
      .find((l) => l.startsWith("ExecStart="));
    expect(execStart).toBeDefined();
    const args = (execStart ?? "").slice("ExecStart=".length).split(" ");
    expect(args.slice(1)).toEqual(["run", "--profile", "work"]);
    expect(resolveRunConfigPath(args.slice(2)).configPath).toBe(
      configPathForProfile("work"),
    );

    // launchd: separate <string> program arguments.
    const darwin = buildUnit("darwin", "/bin/shannon-gateway", "work");
    expect(darwin.contents).toContain("<string>run</string>");
    expect(darwin.contents).toContain("<string>work</string>");

    // schtasks: one joined Command string carrying the same args.
    const win = buildUnit("win32", "C:\\bin\\shannon-gateway.exe", "work");
    expect(win.contents).toMatch(/run --profile work</);
    expect(resolveRunConfigPath(["--profile", "work"]).configPath).toBe(
      configPathForProfile("work"),
    );
  });
});
