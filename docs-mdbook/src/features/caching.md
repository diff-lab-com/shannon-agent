# Prompt Caching

Shannon Code implements three-layer Anthropic cache breakpoint injection to minimize token costs.

## How Prompt Caching Works

Anthropic's API supports cache breakpoints that allow reusing previously computed prompt prefixes. Shannon injects breakpoints at three strategic locations:

### Layer 1: System Prompt
System content blocks use `SystemContentBlock::cached()` to mark the system prompt as cacheable.

### Layer 2: Tool Definitions
The last `ToolDefinition` in the tool list receives a `cache_control` field, marking the entire tool schema as cacheable.

### Layer 3: User Message
`inject_cache_control_on_last_block()` adds a cache breakpoint to the last content block of the last user message.

## Result

For a typical conversation:
- **First turn**: Full prompt processed (cache miss)
- **Subsequent turns**: Only new content is processed (cache hit for system prompt + tools + prior messages)

This can reduce costs by 80-90% and latency by 5-10x for long conversations.

## Compatibility

- **Anthropic**: Full three-layer caching
- **OpenAI**: Caching handled by the provider (automatic)
- **Ollama**: No caching (local inference)
