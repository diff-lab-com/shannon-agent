---
title: Other Crates
order: 27
section: crates
---

# Other Crates

## shannon-cli

CLI entry point using clap. Handles:
- Argument parsing (REPL mode, headless mode, config commands)
- Config loading from all sources
- Deep link URL scheme (`shannon://prompt?text=<encoded>`)
- URL scheme registration (`--register-url-scheme`)

## shannon-skills

Skill system for command templates. Skills are:
- Loaded from plugin manifests
- Registered as `PromptCommand` in the command registry
- Invoked via slash command name matching the trigger

## shannon-types

Shared type definitions re-exported by `shannon-core`. Small crate (~22 tests) providing common types used across the workspace.

## shannon-tool-interface

Defines the `Tool` trait that all tools implement. The interface separates tool definition from implementation, allowing tools from `shannon-tools`, MCP servers, and plugins to be treated uniformly.

## shannon-codegen

Code generation utilities including:
- Repository map generation (`repomap.rs`)
- Performance benchmarks for codegen operations

## shannon-agent

Single agent runtime (binary crate). Provides a standalone agent process that can be spawned by `shannon-agents` for multi-agent scenarios.

## shannon-desktop

Tauri desktop app — currently scaffolded with TODO stubs. Future home for the GUI version.
