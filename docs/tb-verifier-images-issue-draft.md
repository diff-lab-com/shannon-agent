# Upstream issue draft — terminal-bench-2.1 verifier images missing uv bootstrap

> 收件对象：harbor-framework/terminal-bench（或 TB 2.1 数据集维护方）
> 用途：轻量推动（一封 issue），不发代码。以下为英文 issue 正文草稿。

---

**Title:** TB 2.1 verifier containers fail when the runtime has no network egress to github.com/astral.sh (uv bootstrap missing from verifier image)

**Summary**

When running the full `terminal-bench-2-1` dataset through a custom agent harness, 9–12 of 89 tasks fail in the **verifier phase** — not because the agent's patch is wrong, but because the verifier container tries to bootstrap `uv` / `python-build-standalone` from the network at judgment time and the egress is unavailable, rate-limited, or blocked:

```
failed to download https://github.com/astral-sh/uv/releases/download/0.7.13/uv-x86_64-unknown-linux-gnu.tar.gz
curl: (28) Failed to connect to github.com port 443 after 134795 ms
```

Affected task verifiers we observed (all reward=0 with verifier-phase download errors, agent phase completed normally):
`fix-git`, `fix-ocaml-gc`, `polyglot-rust-c`, `prove-plus-comm`, `overfull-hbox`, `password-recovery`, `regex-log`, `sqlite-db-truncate`, `video-processing`, `extract-moves-from-video`, `financial-document-processor` — plus 3 verifier timeouts while pulling `torch` CUDA wheels (`mteb-retrieve`, `torch-tensor-parallelism`, `pytorch-model-cli`).

**Impact**

Roughly **12–15% of the dataset** measures the *runner's* network egress rather than the *agent's* capability. Scores between harnesses become incomparable when their network conditions differ.

**Suggested directions**

1. Pre-bake `uv` (+ a pinned `python-build-standalone`) into the verifier images for the affected tasks, the same way the agent-side images pin their toolchains; or
2. Document/ship a shared "verifier base" image with uv + common wheels pre-installed; or
3. Support an offline wheel cache mounted into verifier containers.

Happy to share our per-task failure logs if useful.

---

**入库说明**：这是「TB 推动轻量做」的全部动作——提交到 TB 仓库 issue 区即可（不在本仓库），本仓库只需保留此草稿。
