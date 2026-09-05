"""Harbor agent adapter for Shannon Code (dev-HEAD binary, GLM anchor).

Lets `harbor run` drive the Shannon CLI headless inside task containers:

    PYTHONPATH=scripts/eval/harbor-adapter \
    SHANNON_HARBOR_BIN=/path/to/shannon \
    harbor run -d terminal-bench@2.1 \
        -a shannon_harbor_agent:Shannon \
        -m zhipu-coding-plan/glm-5.3-flash \
        --ae SHANNON_API_KEY=$SHANNON_API_KEY \
        -n 89

Model name convention mirrors the other installed agents: `provider/model`
(zhipu-coding-plan/glm-5.3-flash). A bare model id falls back to the
SHANNON_PROVIDER env var, defaulting to zhipu-coding-plan so the anchor
always matches the run-batch/wrapper-glm baseline.

The binary evaluated is the LOCAL dev build pointed at by SHANNON_HARBOR_BIN
(no release download — anchor integrity requires the exact dev HEAD), and
`shannon --version` output is captured into the trial's agent info.
"""

import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.installed.base import (
    BaseInstalledAgent,
    CliFlag,
    with_prompt_template,
)
from harbor.agents.model_connection import (
    ModelConnectionSpec,
)
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

DEFAULT_SHANNON_BIN = (
    "/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon"
)
DEFAULT_PROVIDER = "zhipu-coding-plan"
DEFAULT_MODEL = "glm-5.3-flash"


class Shannon(BaseInstalledAgent):
    """Installs the local Shannon dev binary into the task container and runs
    the task instruction headless (`shannon -p`), NDJSON streamed to
    /logs/agent/shannon.ndjson. Verdicts stay with the task's own tests."""

    MODEL_CONNECTION = ModelConnectionSpec(
        api_key_envs=("SHANNON_API_KEY",),
        base_url_envs=("SHANNON_BASE_URL",),
    )

    CLI_FLAGS = [
        CliFlag("max_turns", cli="--max-turns", type="int"),
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
        return Path(os.environ.get("SHANNON_HARBOR_BIN", DEFAULT_SHANNON_BIN))

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        local_bin = self._local_bin
        if not local_bin.is_file():
            raise RuntimeError(
                f"Shannon dev binary not found at {local_bin} — set "
                "SHANNON_HARBOR_BIN to the built engine (anchor integrity "
                "forbids silently falling back to a release download)"
            )
        # docker-compose environments resolve to `docker cp` under the hood.
        await environment.upload_file(local_bin, "/tmp/shannon-upload")
        await self.exec_as_root(
            environment,
            command=(
                "install -m 0755 /tmp/shannon-upload /usr/local/bin/shannon && "
                "rm -f /tmp/shannon-upload && shannon --version"
            ),
        )

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        pass

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        access = self.model_connection
        if not access.api_key:
            raise RuntimeError(
                "no SHANNON_API_KEY resolved — pass "
                "--ae SHANNON_API_KEY=... to harbor run"
            )

        model_name = self.model_name or DEFAULT_MODEL
        if "/" in model_name:
            provider, model = model_name.split("/", 1)
        else:
            provider = os.environ.get("SHANNON_PROVIDER", DEFAULT_PROVIDER)
            model = model_name
        if access.base_url:
            provider = os.environ.get("SHANNON_PROVIDER", provider)

        env = {
            **access.env,
            "SHANNON_API_KEY": access.api_key,
        }
        if access.base_url:
            env["SHANNON_BASE_URL"] = access.base_url

        escaped_instruction = shlex.quote(instruction)
        cli_flags = self.build_cli_flags()
        extra_flags = (cli_flags + " ") if cli_flags else ""

        await self.exec_as_agent(
            environment,
            command=(
                "shannon "
                f"--provider {shlex.quote(provider)} "
                f"--model {shlex.quote(model)} "
                "--disallowed-tools WebFetch --disallowed-tools WebSearch "
                "--output-format json-stream "
                f"{extra_flags}"
                f"-p {escaped_instruction} "
                "> /logs/agent/shannon.ndjson 2> /logs/agent/shannon.stderr"
            ),
            env=env,
        )
