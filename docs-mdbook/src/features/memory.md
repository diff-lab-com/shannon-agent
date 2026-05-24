# Memory

Shannon Code includes a persistent memory system that stores information across conversations.

## Memory Store

The `MemoryStore` manages memories with:
- **Jaccard similarity deduplication** — Prevents storing duplicate memories
- **Semantic categorization** — Organizes memories by topic
- **Automatic pruning** — Removes stale entries based on age and relevance

## Auto-Dream Service

The `AutoDreamService` extracts memories from conversations:
- Identifies important facts, decisions, and patterns
- Consolidates related memories
- Runs automatically at the end of each session

## Memory Consolidator

The `MemoryConsolidator` merges and prunes memories:
- Deduplicates similar memories
- Merges related entries
- Prunes entries older than a configurable retention period

## /memory Command

Manage memories interactively:

```
/memory list              — Show all memories
/memory search <query>    — Search memories
/memory save <text>       — Save a new memory
/memory delete <id>       — Delete a memory
/memory consolidate       — Merge and prune memories
```
