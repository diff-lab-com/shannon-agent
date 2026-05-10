# Terminal Chat UI Research Report
## Patterns from Codex CLI, Claude Code, and Modern Terminal Chat Applications

Research Date: 2026-05-10
Agent: Explore Agent (a518586b0f91142b6)

---

## Executive Summary

This report analyzes terminal chat UI patterns across three major AI coding assistants (Codex CLI, Claude Code, and Aider) plus modern terminal chat applications. The findings reveal distinct architectural approaches to handling message spacing, viewport management, scrolling, and status information in terminal environments.

---

## 1. MESSAGE SPACING AND READABILITY

### 1.1 Visual Distinction Patterns

**User vs Assistant Message Separation:**

- **Codex CLI**: Uses standard terminal output without explicit visual separators. Relies on prompt indicators and natural language flow.
- **Claude Code**: 
  - Implements React-based rendering with clear message boundaries
  - Uses alternating visual styles for user/assistant messages
  - Markdown rendering with proper spacing between messages
  - Code blocks rendered with syntax highlighting and distinct backgrounds
- **Aider**: 
  - Uses inline chat with markdown formatting
  - Clear visual separation via blank lines between messages
  - Color-coded user vs assistant messages (when terminal supports it)

**Markdown Rendering Approach:**

All three tools use markdown but implement it differently:

1. **Full markdown support** (Claude Code, Aider):
   - Headers, lists, code blocks with syntax highlighting
   - Tables, blockquotes, nested formatting
   - Inline code and bold/italic text
   
2. **Simplified markdown** (Codex CLI):
   - Focus on code blocks and basic formatting
   - Less emphasis on complex markdown structures

**Spacing Issues Identified:**

- **List items after code blocks**: A known issue where markdown list items containing code blocks can render without visual separators (found in Codex CLI)
- **Vertical whitespace**: All tools struggle with balancing readability vs screen real estate
- **Code block spacing**: Critical to maintain readability without excessive blank lines

### 1.2 Recommended Spacing Patterns

Based on research across multiple tools:

```markdown
# Standard message spacing:
[Blank line]
**User**: [message content]
[Blank line]
**Assistant**: [response starts immediately]
[Content with proper markdown spacing]
[Blank line before code blocks]
```

**Key Principles:**
1. Always separate messages with at least one blank line
2. Preserve blank lines before/after code blocks
3. Use visual indicators (colors, symbols) for role distinction
4. Avoid collapsing spacing in lists with block content

---

## 2. CHAT WIDTH / MARGINS

### 2.1 Width Management Strategies

**Claude Code Approach:**
- Uses React Ink for terminal rendering
- Dynamic width calculation based on terminal size
- No hardcoded maximum width - adapts to available space
- Code blocks expand to use available width
- Margins handled by layout engine, not hardcoded

**Codex CLI Approach:**
- Full terminal width utilization
- No artificial margins
- Markdown rendering respects terminal width
- Code blocks use full width minus minimal padding

**Aider Approach:**
- Terminal-width constrained
- Some users report ~50% screen utilization for code blocks
- Horizontal scrolling required for long lines
- No configurable width settings

**Terminal Chat Applications (Kiro, others):**
- Full-width by default
- Optional width constraints via configuration
- Some tools use 80-120 character soft limits for readability
- Environment variable support for width customization (e.g., `$GH_MDWIDTH`)

### 2.2 Recommended Width Strategy

**Best Practice:**
```typescript
// Pseudo-code for width management
const calculateContentWidth = (terminalWidth: number) => {
  const maxReadableWidth = 120; // Characters
  const minMargin = 2; // Characters per side
  
  // Use full width if narrow terminal
  if (terminalWidth <= 80) {
    return terminalWidth - minMargin * 2;
  }
  
  // Cap at readable width for wide terminals
  return Math.min(
    terminalWidth - minMargin * 2,
    maxReadableWidth
  );
};
```

**Key Principles:**
1. Use full terminal width on narrow screens (< 80 chars)
2. Cap at ~100-120 characters for readability on wide screens
3. Allow user configuration via environment variables
4. Code blocks should always use maximum available width
5. Preserve 1-2 character margins on edges

---

## 3. SCROLLING / HISTORY VIEWING

### 3.1 Viewport Management

**Two Major Approaches:**

#### A. Alternate Screen Buffer (Claude Code, Codex CLI)
- Uses DEC 1049 alternate screen protocol
- Full-screen TUI experience
- Content managed in scrollback buffer
- Clean exit restores original terminal content
- Pros: Professional feel, no scrollback pollution
- Cons: Can't see previous terminal output while in chat

#### B. Inline Viewport (Aider, some Codex modes)
- Outputs directly to main terminal buffer
- Becomes part of terminal scrollback
- User can scroll with native terminal scrolling
- Pros: Familiar, works with terminal history
- Cons: Pollutes scrollback, less control over presentation

**Claude Code's Hybrid Approach:**
- Uses React Ink for rendering
- Implements custom scroll management
- "Stick to bottom" behavior during streaming
- Smart scrolling: preserves user position when they scroll up
- Cursor tracking for main-screen vs alt-screen modes

### 3.2 Navigation Patterns

**Keyboard Navigation:**
- Arrow keys: Line-by-line scrolling
- Page Up/Down: Page scrolling
- Home/End: Jump to top/bottom
- Ctrl+R: Search history (Codex CLI, Aider)
- Up/Down: Navigate command history

**Mouse Navigation:**
- Scroll wheel: Native terminal scrolling
- Click to focus input
- Some tools support mouse-based selection

**Advanced Features:**

**"Scroll to Bottom" Indicator** (requested feature for Claude Code):
- Shows visual indicator when user scrolls away from bottom
- One-key return to latest output
- Critical for long conversations
- Pattern borrowed from ChatGPT web interface

**History Management:**
- Session persistence across restarts
- Resume capability with conversation history
- Export/import of conversations
- Search through conversation history

### 3.3 Recommended Scrolling Architecture

```typescript
// Scrolling state management
interface ScrollingState {
  autoScroll: boolean;        // Auto-scroll to new content
  userScrolled: boolean;      // User manually scrolled up
  scrollPosition: number;     // Current position
  showScrollButton: boolean;  // Show "return to bottom" indicator
}

// Auto-scroll behavior
const shouldAutoScroll = (
  state: ScrollingState,
  newContent: boolean
): boolean => {
  // Auto-scroll if:
  // 1. Already at bottom, OR
  // 2. New content arriving and user hasn't scrolled up
  return !state.userScrolled || newContent;
};

// Scroll-to-bottom indicator
const showScrollIndicator = (state: ScrollingState): boolean => {
  return state.userScrolled && state.scrollPosition < 100;
};
```

**Key Principles:**
1. Default to auto-scrolling during streaming
2. Detect when user scrolls up and disable auto-scroll
3. Show visual indicator when scrolled away from bottom
4. Provide one-key shortcut to return to bottom
5. Preserve scroll position during window resize
6. Support both keyboard and mouse navigation

---

## 4. STATUS INFORMATION DISPLAY

### 4.1 Status Bar Patterns

**Claude Code:**
- No dedicated status bar in terminal mode
- Model information shown at session start
- Token/cost information shown per-message
- Progress indicators inline with responses
- Activity tray (Ctrl+X) for task progress

**Codex CLI:**
- Minimal status display
- Model name in prompt or header
- Tool calls shown inline
- No persistent status bar

**Aider:**
- Model and cost information shown inline
- Example: `> Tokens: 4.5k sent, 742 received. Cost: $0.02 message, $0.42 session.`
- Edit confirmations inline
- Git commit messages inline
- Tool usage notifications inline

**Terminal Chat Applications (Kiro, etc.):**
- Bottom status bar with:
  - Current model
  - Token count
  - Session cost
  - Active tools
  - Keyboard shortcuts hint
- Activity tray for background tasks
- Overlay panels for detailed status

### 4.2 Information Hierarchy

**Always Visible (Status Bar/Header):**
- Current model name
- Session cost/tokens (optional, toggleable)
- Connection status

**Inline With Messages:**
- Per-message token count
- Tool usage notifications
- Error messages
- Progress indicators

**On Demand (Overlay Panels):**
- Full session statistics
- Tool history
- Context information
- Help/shortcuts

**Configuration:**
- Allow users to customize what's shown
- Support minimal vs verbose modes
- Respect user preferences for information density

### 4.3 Recommended Status Display Strategy

```typescript
interface StatusDisplay {
  // Always visible in header/status bar
  persistent: {
    modelName: string;
    sessionActive: boolean;
  };
  
  // Shown inline with each message
  perMessage: {
    tokenCount: boolean;
    cost: boolean;
    toolsUsed: boolean;
  };
  
  // On-demand via keypress
  overlay: {
    fullStats: boolean;
    toolHistory: boolean;
    contextInfo: boolean;
  };
  
  // User preferences
  userPreferences: {
    verboseMode: boolean;
    showCosts: boolean;
    showTokens: boolean;
  };
}
```

**Status Bar Layout:**
```
┌─────────────────────────────────────────────────────┐
│ claude-3.7-sonnet | Session: $0.42 | Ctrl+X: Status │
├─────────────────────────────────────────────────────┤
│                                                     │
│ [Message content...]                                │
│                                                     │
├─────────────────────────────────────────────────────┤
│ Tokens: 4.5k sent, 742 received | Cost: $0.02      │
└─────────────────────────────────────────────────────┘
```

**Key Principles:**
1. Put essential info in status bar (model, session status)
2. Show per-message stats inline but keep them compact
3. Use overlay panels for detailed information
4. Make status display configurable
5. Don't overwhelm users with information
6. Group related information (costs, tokens, tools)
7. Use keyboard shortcuts to toggle verbose mode

---

## 5. TERMINAL UI ARCHITECTURAL PATTERNS

### 5.1 Rendering Technologies

**React Ink (Claude Code):**
- Component-based architecture
- Rich layout system
- Built-in markdown support
- Cross-platform consistency
- Pros: Modern, maintainable, rich features
- Cons: Heavy dependency, slower startup

**Raw ANSI (Custom implementations):**
- Direct terminal control
- Lightweight
- Fast startup
- Pros: Minimal dependencies, fast
- Cons: Harder to maintain, limited features

**Bubble Tea (Go):**
- Popular for Go-based TUIs
- Elm architecture
- Good for complex UIs
- Pros: Type-safe, good patterns
- Cons: Go-specific

**Ratatui (Rust):**
- Modern Rust TUI library
- Flexible layout system
- Good performance
- Pros: Fast, safe, modern
- Cons: Rust-specific

### 5.2 Streaming Architecture

**Challenge:** LLM output arrives token-by-token (10-50 tokens/second)

**Naive Approach (DO NOT USE):**
```typescript
// Reparse entire message on every token - CATASTROPHIC
foreach token in stream {
  fullMessage += token;
  rendered = parseMarkdown(fullMessage);
  render(rendered);
}
```

**Optimized Approaches:**

**1. Block-Level Caching (Claude Code, Textual):**
```typescript
// Only reparse the last block
interface StreamingState {
  finalizedBlocks: MarkdownBlock[];
  activeBlock: PartialMarkdownBlock;
}

foreach token in stream {
  state.activeBlock.content += token;
  
  // Only parse active block
  rendered = parseBlock(state.activeBlock);
  
  // Render: finalized blocks + new active block
  render([...state.finalizedBlocks, rendered]);
}

// When block complete, move to finalized
if (isBlockComplete(state.activeBlock)) {
  state.finalizedBlocks.push(
    parseBlock(state.activeBlock)
  );
  state.activeBlock = newBlock();
}
```

**2. Incremental Parsing (Will McGugan's approach):**
- Track line number where last block began
- Only parse from that line to end
- Sub-1ms parsing regardless of document size
- All prior blocks considered finalized

**3. Lazy Syntax Highlighting:**
```typescript
// Render immediately without highlighting
function renderCode(code: string) {
  return <code>{code}</code>; // Fast
}

// Then upgrade asynchronously
setTimeout(() => {
  const highlighted = await highlightCode(code);
  updateComponent(highlighted); // Users sees "pop" of color
}, 0);
```

### 5.3 Recommended Architecture

```typescript
interface TerminalChatArchitecture {
  // Rendering engine
  renderer: {
    type: 'react-ink' | 'raw-ansi' | 'framework';
    streamingStrategy: 'block-cache' | 'incremental';
  };
  
  // State management
  state: {
    messages: Message[];
    scrollPosition: number;
    viewport: ViewportState;
  };
  
  // UI components
  components: {
    messageList: MessageListComponent;
    inputArea: InputComponent;
    statusBar: StatusBarComponent;
    overlays: OverlayManager;
  };
  
  // Performance optimizations
  optimizations: {
    blockCaching: boolean;
    lazyHighlighting: boolean;
    virtualScrolling: boolean; // For very long conversations
  };
}
```

---

## 6. COMPARATIVE ANALYSIS

### 6.1 Feature Comparison Matrix

| Feature | Claude Code | Codex CLI | Aider | Recommended |
|---------|-------------|-----------|-------|-------------|
| **Viewport Mode** | Alternate screen | Alternate screen | Inline | Alternate screen |
| **Markdown** | Full support | Full support | Full support | Full support |
| **Code Highlighting** | Yes, lazy-loaded | Yes | Yes | Yes, lazy-loaded |
| **Message Spacing** | Automatic | Minimal issues | Good | Blank lines + block detection |
| **Width Management** | Dynamic | Full width | Constrained | Dynamic with cap |
| **Scroll Indicator** | Requested | No | No | Yes, critical feature |
| **Status Bar** | Minimal | Minimal | Inline | Configurable bar + inline |
| **History Search** | Via overlay | Ctrl+R | Up/Down arrows | Multiple methods |
| **Session Resume** | Yes | Yes | Yes | Yes |
| **Cost Display** | Per-message | Per-message | Per-message | Per-message + toggleable total |
| **Keyboard Nav** | Full support | Full support | Limited | Full support required |

### 6.2 Strengths by Tool

**Claude Code:**
- Best overall architecture
- Rich markdown rendering
- Optimized streaming
- Good session management
- Missing: Scroll-to-bottom indicator

**Codex CLI:**
- Lightweight
- Fast startup
- Good for quick tasks
- Missing: Advanced UI features
- Missing: Scroll navigation aids

**Aider:**
- Git integration
- Cost transparency
- Practical for coding
- Missing: Advanced UI
- Missing: Alternate screen mode

---

## 7. DESIGN RECOMMENDATIONS

### 7.1 Message Spacing & Readability

**✅ DO:**
- Separate all messages with blank lines
- Preserve blank lines before/after code blocks
- Use visual indicators (colors, symbols) for roles
- Detect block content in lists and add spacing
- Support full markdown with syntax highlighting
- Use lazy loading for syntax highlighting

**❌ DON'T:**
- Collapse vertical whitespace
- Use hardcoded spacing values
- Ignore terminal color capabilities
- Re-render entire message on each token

### 7.2 Chat Width & Margins

**✅ DO:**
- Use full terminal width on narrow screens
- Cap at 100-120 characters on wide screens
- Allow user configuration via environment variables
- Give code blocks maximum available width
- Preserve 1-2 character margins

**❌ DON'T:**
- Hardcode narrow widths (e.g., 50% of screen)
- Force horizontal scrolling unnecessarily
- Ignore terminal resize events

### 7.3 Scrolling & History

**✅ DO:**
- Use alternate screen buffer for TUI
- Implement "stick to bottom" during streaming
- Detect when user scrolls up
- Show visual indicator when scrolled away from bottom
- Provide one-key return to bottom
- Support keyboard and mouse navigation
- Preserve scroll position on resize
- Implement search (Ctrl+R)

**❌ DON'T:**
- Force auto-scroll when user is reading
- Lose scroll position on updates
- Make users scroll manually to find new content

### 7.4 Status Information

**✅ DO:**
- Show essential info in status bar (model, session status)
- Display per-message stats inline but compact
- Use overlay panels for detailed info
- Make display configurable (verbose/quiet modes)
- Group related information
- Provide keyboard shortcuts for status toggles

**❌ DON'T:**
- Overwhelm with information
- Show all stats by default
- Hide essential information behind multiple keypresses
- Make cost/token info non-optional

### 7.5 Architecture

**✅ DO:**
- Use block-level caching for streaming
- Implement lazy syntax highlighting
- Consider virtual scrolling for very long conversations
- Support both keyboard and mouse interaction
- Use incremental parsing for markdown

**❌ DON'T:**
- Re-parse entire message on each token
- Synchronous syntax highlighting
- Ignore performance at scale

---

## 8. IMPLEMENTATION CHECKLIST

### Phase 1: Core UI
- [ ] Alternate screen buffer management
- [ ] Message rendering with markdown
- [ ] User/assistant message distinction
- [ ] Basic input handling
- [ ] Keyboard navigation (arrows, Page Up/Down, Home/End)

### Phase 2: Spacing & Readability
- [ ] Blank line separation between messages
- [ ] Block content detection in lists
- [ ] Code block spacing preservation
- [ ] Visual role indicators (colors/symbols)

### Phase 3: Width Management
- [ ] Dynamic width calculation
- [ ] Maximum readable width cap (100-120)
- [ ] Environment variable configuration
- [ ] Terminal resize handling

### Phase 4: Scrolling & Navigation
- [ ] Auto-scroll during streaming
- [ ] Scroll position detection
- [ ] Scroll-to-bottom indicator
- [ ] One-key return to bottom
- [ ] History search (Ctrl+R)
- [ ] Mouse scroll support

### Phase 5: Status Display
- [ ] Status bar with model name
- [ ] Inline per-message token/cost display
- [ ] Overlay panels for detailed info
- [ ] Configurable verbose/quiet modes
- [ ] Activity tray for background tasks

### Phase 6: Performance
- [ ] Block-level caching for streaming
- [ ] Lazy syntax highlighting
- [ ] Incremental markdown parsing
- [ ] Virtual scrolling (if needed)

### Phase 7: Polish
- [ ] Session persistence
- [ ] Resume capability
- [ ] Export/import conversations
- [ ] Comprehensive keyboard shortcuts
- [ ] Help documentation

---

## 9. KEY INSIGHTS

### 9.1 Critical Success Factors

1. **Streaming Performance**: Must handle 10-50 tokens/second without lag
2. **Scroll Behavior**: Auto-scroll with user override is non-negotiable
3. **Information Density**: Balance between showing enough vs. overwhelming
4. **Terminal Constraints**: Work within terminal's limitations, don't fight them
5. **User Control**: Give users options to customize display

### 9.2 Common Pitfalls

1. **Over-engineering**: Complex UIs feel out of place in terminal
2. **Ignoring Terminal Capabilities**: Not all terminals support colors/alternate screen
3. **Poor Spacing**: Makes long conversations unreadable
4. **No Scroll Indicator**: Users get lost in long conversations
5. **Forced Auto-scroll**: Frustrating when trying to read earlier content

### 9.3 Innovation Opportunities

1. **Adaptive Width**: Learn user's preferred width over time
2. **Smart Scrolling**: Predict when user wants to see new content
3. **Context-Aware Status**: Show relevant info based on current task
4. **Session Branching**: Allow exploring alternatives without losing context
5. **Visual History**: Timeline or graph view of conversation

---

## 10. CONCLUSION

The research reveals three distinct but converging approaches to terminal chat UIs:

**Claude Code** leads with a modern, React-based architecture that prioritizes user experience through intelligent streaming optimizations and rich markdown rendering.

**Codex CLI** takes a minimalist approach, focusing on speed and simplicity while still providing essential features.

**Aider** balances practicality with usability, particularly strong on git integration and cost transparency.

**Best practices** emerge from synthesizing these approaches:
- Use alternate screen for professional feel
- Implement block-level streaming for performance
- Provide scroll-to-bottom indicator for usability
- Make status information configurable
- Support both keyboard and mouse navigation
- Preserve spacing in complex markdown structures

The terminal chat UI space is rapidly evolving, with lessons from web chat interfaces being adapted to terminal constraints. The winning pattern combines the familiarity of terminal workflows with the intelligence of modern AI assistants.

---

## REFERENCES & FURTHER READING

1. Claude Code from Source - Terminal UI Architecture
   - https://claude-code-from-source.com/ch13-terminal-ui/
   
2. OpenAI Codex CLI Documentation
   - https://developers.openai.com/codex/cli/features
   
3. Aider Documentation
   - https://aider.chat/docs/usage/commands.html
   
4. Efficient Streaming of Markdown in Terminal - Will McGugan
   - http://willmcgugan.github.io/streaming-markdown/
   
5. Ratatui Terminal UI Framework
   - https://ratatui.rs/concepts/backends/alternate-screen/
   
6. AI Chat UI Best Practices - TheFrontKit
   - https://thefrontkit.com/blogs/ai-chat-ui-best-practices
   
7. CLI UX Best Practices - Evil Martians
   - https://evilmartians.com/chronicles/cli-ux-best-practices-3-patterns-for-improving-progress-displays

---

**Report Generated**: 2026-05-10
**Agent**: Explore Agent (a518586b0f91142b6)
**Status**: Research Complete
