"""Pier agent adapter for Shannon (dev binary, GLM anchor) — DeepSWE v1.1.

Lets `pier run` drive the Shannon CLI headless inside task containers:

    PYTHONPATH=scripts/eval/pier-adapter \
    SHANNON_PIER_BIN=/path/to/shannon \
    pier run -p ~/eval-corpora/deep-swe/tasks \
        --agent-import-path shannon_pier_agent:Shannon \
        -m zhipu-coding-plan/glm-5.3-flash \
        --ae SHANNON_API_KEY=$SHANNON_API_KEY \
        -n 3

Design contract (docs/research/pier-adapter-notes-2026-09.md, P0-4):
  - Model convention mirrors the other installed agents: `provider/model`
    (zhipu-coding-plan/glm-5.3-flash). A bare model id falls back to
    SHANNON_PROVIDER env, defaulting to zhipu-coding-plan so the anchor
    matches the wrapper-glm / run-batch baseline. The API id is passed
    through verbatim — batch-5 RCA rule: never "normalize" it.
  - The evaluated binary is the LOCAL dev build (anchor integrity forbids
    release downloads); libc probe picks a musl build for musl images.
  - Fairness: instruction.md carries the task contract (including "commit
    everything"); the adapter never commits, never rewrites prompts, and
    allowlists only the LLM endpoint host. Same harness for every agent.
"""

import asyncio
import json
import os
import shlex
import tempfile
from pathlib import Path
from typing import override

from pier.agents.installed.base import (
    BaseInstalledAgent,
    CliFlag,
    NonZeroAgentExitCodeError,
    with_prompt_template,
)
from pier.environments.base import BaseEnvironment
from pier.models.agent.context import AgentContext
from pier.models.agent.install import AgentInstallSpec, InstallStep
from pier.models.agent.network import NetworkAllowlist

DEFAULT_SHANNON_BIN = (
    "/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon"
)
DEFAULT_PROVIDER = "zhipu-coding-plan"
DEFAULT_MODEL = "glm-5.3-flash"
# GLM-5.3 thinking has normal mid-reasoning silences up to ~312s (RCA
# 2026-09-07); a watchdog below that killed healthy streams and the engine
# retry re-thought from scratch (rc=3 death spiral). 420s covers observed
# silences with margin. Overridable via --ae SHANNON_STREAM_IDLE_SECS=N.
DEFAULT_STREAM_IDLE_SECS = "420"
# DeepSWE long-horizon: official mini-swe-agent runs average ~123 steps;
# the engine's default max_turns=20 would truncate them. 150 = steps + margin.
DEFAULT_MAX_TURNS = 150


class Shannon(BaseInstalledAgent):
    """Installs the local Shannon dev binary into the task container and runs
    the task instruction headless (`shannon -p`), NDJSON streamed to
    /logs/agent/shannon.ndjson. Verdicts stay with the task's own verifier."""

    CLI_FLAGS = [
        CliFlag("max_turns", cli="--max-turns", type="int", default=DEFAULT_MAX_TURNS),
    ]

    @staticmethod
    @override
    def name() -> str:
        return "shannon"

    @override
    def get_version_command(self) -> str | None:
        return "shannon --version"

    @property
    def _local_bin(self) -> Path:
        for var in ("SHANNON_PIER_BIN", "SHANNON_HARBOR_BIN"):
            if os.environ.get(var):
                return Path(os.environ[var])
        return Path(DEFAULT_SHANNON_BIN)

    @property
    def _local_musl_bin(self) -> Path:
        return Path(
            os.environ.get(
                "SHANNON_PIER_MUSL_BIN",
                str(self._local_bin.parent.parent
                    / "x86_64-unknown-linux-musl" / "release" / "shannon"),
            )
        )

    def install_spec(self) -> AgentInstallSpec:
        # Placeholder only: the validator requires >=1 step. The real install
        # (binary upload with libc probe) lives in install() below — a binary
        # upload cannot be expressed as a build-time text step.
        return AgentInstallSpec(
            agent_name=self.name(),
            steps=[InstallStep(run="mkdir -p /usr/local/bin", user="root")],
            verification_command=self.get_version_command(),
        )

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        # The default dev build is dynamically linked against host glibc and
        # dies on alpine/musl images (harbor adapter RCA 2026-09-06). DeepSWE
        # images are swe-bench-style Debian (glibc), but keep the probe so the
        # adapter stays corpus-agnostic. Never silently fall back to a release
        # download — anchor integrity requires the exact dev HEAD.
        try:
            probe = await self.exec_as_root(
                environment,
                command=(
                    "if [ -e /lib/ld-musl-x86_64.so.1 ]; then echo musl; "
                    "else ldd --version 2>/dev/null | head -1; fi"
                ),
            )
            libc_info = str(getattr(probe, "stdout", "") or "")
        except Exception:
            libc_info = ""
        local_bin = self._local_bin
        if "musl" in libc_info:
            musl_bin = self._local_musl_bin
            if musl_bin.is_file():
                local_bin = musl_bin
        if not local_bin.is_file():
            raise RuntimeError(
                f"Shannon dev binary not found at {local_bin} — set "
                "SHANNON_PIER_BIN to the built engine (anchor integrity "
                "forbids silently falling back to a release download)"
            )
        await environment.upload_file(local_bin, "/tmp/shannon-upload")
        await self.exec_as_root(
            environment,
            command=(
                "install -m 0755 /tmp/shannon-upload /usr/local/bin/shannon && "
                "rm -f /tmp/shannon-upload && shannon --version"
            ),
        )

    @override
    async def setup(self, environment: BaseEnvironment) -> None:
        # Pier inlines install_spec() steps into the derived agent image and
        # then marks the agent preinstalled (environment.agent_install_spec),
        # skipping the default install() path — but a binary upload cannot be
        # expressed as a build-time text step, so the placeholder spec installs
        # nothing. Force the real install on every trial.
        await environment.exec(command="mkdir -p /installed-agent", user="root")
        await self.install(environment)
        if self._version is None:
            version_cmd = self.get_version_command()
            if version_cmd:
                try:
                    version_result = await environment.exec(command=version_cmd)
                    if version_result.return_code == 0 and version_result.stdout:
                        self._version = self.parse_version(version_result.stdout)
                except Exception:
                    pass  # Version detection is best-effort

    @override
    def network_allowlist(self) -> NetworkAllowlist:
        # zhipu-coding-plan endpoint host (open.bigmodel.cn/api/coding/paas/v4).
        # The API-key→JWT signature is computed locally; no other host needed.
        # DeepSWE tasks run agent network_mode=no-network — pier pokes only
        # this hole. models.dev catalog merge is optional at runtime (C4 put
        # glm-5.3-flash in the static catalog); smoke gate verifies startup
        # without it.
        return NetworkAllowlist(domains=["open.bigmodel.cn"])

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        # Pier mounts logs_dir at /logs/agent in the container (mini-swe reads
        # its trajectory back the same way).
        source = self.logs_dir / "shannon.ndjson"
        if not source.is_file():
            self.logger.debug(
                f"Shannon trajectory file {source} does not exist"
            )
            return
        tokens_in = tokens_out = steps = 0
        exit_code = None
        with open(source, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                kind = event.get("event") or event.get("type") or ""
                if kind in ("tool_call", "tool_start"):
                    steps += 1
                elif kind == "done":
                    usage = event.get("usage") or event
                    tokens_in = int(usage.get("tokens_in") or 0)
                    tokens_out = int(usage.get("tokens_out") or 0)
                    exit_code = usage.get("exit_code")
        context.n_input_tokens = tokens_in or None
        context.n_output_tokens = tokens_out or None
        context.n_agent_steps = steps or None
        context.metadata = {"exit_code": exit_code}

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        if not self._get_env("SHANNON_API_KEY"):
            raise RuntimeError(
                "no SHANNON_API_KEY resolved — pass "
                "--ae SHANNON_API_KEY=... to pier run"
            )

        model_name = self.model_name or DEFAULT_MODEL
        if "/" in model_name:
            provider, model = model_name.split("/", 1)
        else:
            provider = os.environ.get("SHANNON_PROVIDER", DEFAULT_PROVIDER)
            model = model_name

        env = {"SHANNON_API_KEY": self._get_env("SHANNON_API_KEY") or ""}
        # Forward host-side SHANNON_* tuning into the container (stream-idle
        # watchdog, pacing, ...) so eval controls apply verbatim.
        for key, value in os.environ.items():
            if key.startswith("SHANNON_") and key not in env:
                env[key] = value
        if self._extra_env:
            env.update(self._extra_env)
        env.setdefault("SHANNON_STREAM_IDLE_SECS", DEFAULT_STREAM_IDLE_SECS)
        # Total per-request wall-clock (engine default 300s) must exceed the
        # content-idle watchdog AND GLM's observed ~312s thinking silences:
        # smoke-2 died rc=3 at turn 8 when one long thinking call tripped the
        # 300s total timeout, killing the run with 42 tool calls uncommitted
        # (empty patch → F2P 0). 1800s backstops the 420s idle watchdog.
        env.setdefault("SHANNON_TIMEOUT", "1800")

        # Web tools off: the official harness (mini-swe-agent) has no web
        # access, and DeepSWE agent containers are no-network except the LLM
        # endpoint — shannon's WebFetch/WebSearch would only burn turns failing
        # (parity with wrapper-glm's eval default; not a capability change).
        disallowed = os.environ.get(
            "SHANNON_DISALLOWED_TOOLS", "WebFetch WebSearch"
        )
        disallowed_flags = ""
        if disallowed.strip():
            tools = " ".join(
                shlex.quote(t) for t in disallowed.split()
            )
            disallowed_flags = f"--disallowed-tools {tools} "

        cli_flags = self.build_cli_flags()
        extra_flags = (cli_flags + " ") if cli_flags else ""

        # Write the prompt to a temp file and pass it via attached
        # `--prompt=$(cat file)`: clap refuses separated values starting with
        # '-' and `-p - < file` assigns a literal "-" (RCA 2026-09-07); the
        # attached = form carries the file's bytes verbatim.
        prompt_filename = "shannon_prompt.txt"
        prompt_target = f"/tmp/{prompt_filename}"
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".txt", delete=False, encoding="utf-8"
        ) as tmp:
            tmp.write(instruction)
            tmp_path = Path(tmp.name)
        try:
            await environment.upload_file(tmp_path, prompt_target)
        finally:
            tmp_path.unlink(missing_ok=True)

        command = (
            "shannon "
            f"--provider {shlex.quote(provider)} "
            f"--model {shlex.quote(model)} "
            "--output-format json-stream "
            f"{disallowed_flags}"
            f"{extra_flags}"
            f"--prompt=\"$(cat {shlex.quote(prompt_target)})\" "
            "> /logs/agent/shannon.ndjson 2> /logs/agent/shannon.stderr"
        )
        # rc=4 (rate-limit) retries at the harness layer: coding-plan windows
        # are bursty and the engine's in-call retry cannot cover an immediate
        # first-call rejection. One retry after 60s (harbor adapter parity).
        for attempt in range(2):
            try:
                await self.exec_as_agent(environment, command=command, env=env)
                return
            except NonZeroAgentExitCodeError as exc:
                if attempt == 0 and "exit 4" in str(exc):
                    self.logger.warning(
                        "shannon exited rc=4 (rate limit); retrying once in 60s"
                    )
                    await asyncio.sleep(60)
                    continue
                raise
