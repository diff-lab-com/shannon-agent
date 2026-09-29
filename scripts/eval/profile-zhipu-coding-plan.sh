#!/usr/bin/env bash
# profile-zhipu-coding-plan.sh — frozen concurrency/pacing profile for
# SWE-bench batches on the zhipu GLM coding-plan channel.
#
# Every value here is measured, not guessed (lite100 batch 2026-09-28,
# docs/eval-findings + FINDINGS.md S7/S8/D3):
#   - 3 concurrent workers: 6-way concurrency tripped coding-plan rate
#     limits (rc=4 death spiral); 3-way ran 100 tasks with zero 429s.
#   - SWE_MIN_DELAY_MS=15000: global start stagger via wrapper pacing state.
#   - SHANNON_STREAM_IDLE_SECS=360: GLM-5.3-flash's max measured thinking
#     silence is 312s — a watchdog below that kills healthy streams.
# Usage: source this file, then launch the driver.
# shellcheck shell=bash

export SHANNON_STREAM_IDLE_SECS=360   # must exceed max thinking silence (312s)
export SWE_MIN_DELAY_MS=15000         # global start stagger across workers
export SWE_AGENT_MAX_TURNS=80
export SWE_AGENT_TIMEOUT_SECS=1800    # NOTE: starves on long-thinking tasks
                                      # (FINDINGS D3) — 2700-3600 recommended
                                      # for flash-tier models pending A/B
export SWE_TEST_TIMEOUT_SECS=1800
export SHANNON_TURN_CHECKPOINT=15
export SHANNON_TOKEN_BUDGET_WARNING=true
export SWE_AGENT_HINT=1

# Optional: explicit thinking toggle for the GLM request (P1-2 knob).
# unset            -> provider default (GLM-5.x: thinking ON)
# SHANNON_THINKING=disabled -> request carries thinking.type=disabled
# export SHANNON_THINKING=disabled

SWE_MAX_CONCURRENCY=3   # informational: the driver must cap workers at this

echo "[profile] zhipu-coding-plan: workers<=${SWE_MAX_CONCURRENCY}, stagger=${SWE_MIN_DELAY_MS}ms, stream-idle=${SHANNON_STREAM_IDLE_SECS}s, agent-timeout=${SWE_AGENT_TIMEOUT_SECS}s"
