import { describe, it, expect } from 'vitest'
import { splitStreamingMarkdown } from '@/lib/streamingMarkdown'

// B3-2 (§三 P1-6): the streaming body is cut into a finalized prefix + active
// tail at the last blank-line boundary that is provably a true block
// boundary. These tests pin the boundary rules — especially the
// counter-examples (open fences, `$$` math spanning blanks, quote interiors,
// loose lists, indented-code interiors) where a naive "split at \n\n" would
// tear a live block in half.

/** Contract every result must uphold, applied to every fixture below. */
function expectWellFormed(text: string) {
  const { prefix, tail } = splitStreamingMarkdown(text)
  expect(prefix + tail).toBe(text)
  // The tail is only empty when the text itself is (the component never
  // renders an empty tail).
  if (text !== '') expect(tail).not.toBe('')
  return { prefix, tail }
}

describe('splitStreamingMarkdown — basic boundaries', () => {
  it('keeps a single paragraph wholly in the tail (nothing finalized)', () => {
    const { prefix, tail } = expectWellFormed('Hello **world**')
    expect(prefix).toBe('')
    expect(tail).toBe('Hello **world**')
  })

  it('finalizes at the blank line between two paragraphs', () => {
    const { prefix, tail } = expectWellFormed('Para one.\n\nPara two.')
    expect(prefix).toBe('Para one.\n\n')
    expect(tail).toBe('Para two.')
  })

  it('cuts at the LAST valid boundary (largest prefix, smallest tail)', () => {
    const { prefix, tail } = expectWellFormed('A\n\nB\n\nC')
    expect(prefix).toBe('A\n\nB\n\n')
    expect(tail).toBe('C')
  })

  it('does not finalize a trailing blank run (nothing after it yet)', () => {
    const { prefix, tail } = expectWellFormed('A\n\n')
    expect(prefix).toBe('')
    expect(tail).toBe('A\n\n')
  })

  it('handles leading blank lines and multi-line blank runs', () => {
    const { prefix, tail } = expectWellFormed('\n\n\nA\n\n\n\nB')
    expect(prefix).toBe('\n\n\nA\n\n\n\n')
    expect(tail).toBe('B')
  })

  it('returns empty parts for empty input', () => {
    expect(splitStreamingMarkdown('')).toEqual({ prefix: '', tail: '' })
  })

  it('keeps headings, lists-as-blocks, and thematic breaks splittable', () => {
    const { prefix, tail } = expectWellFormed('# Title\n\nBody text.\n\n---\n\nMore.')
    expect(prefix).toBe('# Title\n\nBody text.\n\n---\n\n')
    expect(tail).toBe('More.')
  })
})

describe('splitStreamingMarkdown — fenced code (unclosed must never split)', () => {
  const fencedWithBlanks = '```python\ndef f():\n    x = 1\n\n    return x\n'

  it('keeps blank lines inside an OPEN fence out of the boundary set', () => {
    const text = 'Intro para.\n\n' + fencedWithBlanks
    const { prefix, tail } = expectWellFormed(text)
    // The only safe boundary is before the fence; the fence (with its
    // interior blanks) stays wholly in the tail.
    expect(prefix).toBe('Intro para.\n\n')
    expect(tail).toBe(fencedWithBlanks)
  })

  it('never splits when the whole text is an unclosed fence', () => {
    const { prefix, tail } = expectWellFormed(fencedWithBlanks)
    expect(prefix).toBe('')
    expect(tail).toBe(fencedWithBlanks)
  })

  it('lets a CLOSED fence finalize once a blank line follows it', () => {
    const text = '```ts\nconst x = 1\n```\n\nAfter the block.'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('```ts\nconst x = 1\n```\n\n')
    expect(tail).toBe('After the block.')
  })

  it('handles a fence reopened after a closed one', () => {
    const text = '```a\nx\n```\n\nmid\n\n```b\n\nstill open'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('```a\nx\n```\n\nmid\n\n')
    expect(tail).toBe('```b\n\nstill open')
  })

  it('tracks tilde fences too', () => {
    const text = 'before\n\n~~~\ncode\n\nwith blanks\n'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('before\n\n')
    expect(tail).toBe('~~~\ncode\n\nwith blanks\n')
  })

  it('does not treat a ``` line inside an open longer-backtick fence as a close', () => {
    // Opening with ```` (4) requires ≥4 backticks to close; the ``` inside
    // is content, and the blanks under it stay fenced.
    const text = '````md\nnested ``` example\n\nwith blank\n'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('')
    expect(tail).toBe(text)
  })
})

describe('splitStreamingMarkdown — `$$` flow math (may span blank lines)', () => {
  it('never splits inside a multi-paragraph display math block', () => {
    const math = '$$\nx = 1\n\ny = 2\n$$'
    const text = 'Lead.\n\n' + math + '\n\nAfter.'
    const { prefix, tail } = expectWellFormed(text)
    // Math is finalized as a whole only once its closing fence has landed.
    expect(prefix).toBe('Lead.\n\n' + math + '\n\n')
    expect(tail).toBe('After.')
  })

  it('keeps an open math block (closing fence not yet streamed) wholly in the tail', () => {
    const math = '$$\nx = 1\n\ny = 2\n'
    const text = 'Lead.\n\n' + math
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('Lead.\n\n')
    expect(tail).toBe(math)
  })

  it('treats single-line $$x$$ math as self-contained', () => {
    const { prefix, tail } = expectWellFormed('$$E=mc^2$$\n\nAfter.')
    expect(prefix).toBe('$$E=mc^2$$\n\n')
    expect(tail).toBe('After.')
  })

  it('applies the same-length close rule to $ runs', () => {
    // `$$` (2) must not be closed by a single `$` line — probe-verified
    // remark behavior the scanner must mirror.
    const text = '$$\nx\n$\n\nafter'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('')
    expect(tail).toBe(text)
  })
})

describe('splitStreamingMarkdown — tables (never straddle the cut)', () => {
  it('keeps a mid-construction table wholly in the tail', () => {
    const table = '| a | b |\n|---|---|\n| 1 | 2 |'
    const text = 'Intro.\n\n' + table
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('Intro.\n\n')
    expect(tail).toBe(table)
  })

  it('finalizes a completed table as a whole, tail only the next paragraph', () => {
    const table = '| a | b |\n|---|---|\n| 1 | 2 |'
    const text = table + '\n\nAfter the table.'
    const { prefix, tail } = expectWellFormed(text)
    // Whole table in the prefix — never header-in-prefix / rows-in-tail.
    expect(prefix).toBe(table + '\n\n')
    expect(tail).toBe('After the table.')
  })

  it('never leaves a partial table in the prefix across growth', () => {
    // Simulate the table streaming in row by row: at every step the table
    // rows are entirely inside the tail, or — once the terminator blank has
    // arrived — entirely inside the prefix. Never straddling the cut.
    const rows = ['| a | b |', '|---|---|', '| 1 | 2 |', '| 3 | 4 |']
    let text = 'Intro.\n\n'
    for (const row of rows) {
      text += row + '\n'
      const { prefix } = expectWellFormed(text)
      const finalizedRows = prefix.split('\n').filter(l => l.trim().startsWith('|'))
      const totalRows = text.split('\n').filter(l => l.trim().startsWith('|'))
      if (finalizedRows.length > 0) expect(finalizedRows).toEqual(totalRows)
    }
  })
})

describe('splitStreamingMarkdown — blockquotes (the `>` blank keeps them open)', () => {
  it('does not split a one-quote-two-paragraph blockquote (`> a` / `>` / `> b`)', () => {
    const quote = '> first\n>\n> second'
    const text = quote + '\n\nAfter.'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe(quote + '\n\n')
    expect(tail).toBe('After.')
  })

  it('keeps an open quote with its interior blank wholly in the tail', () => {
    const quote = '> first\n>\n> second'
    const text = 'Lead.\n\n' + quote
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('Lead.\n\n')
    expect(tail).toBe(quote)
  })

  it('splits between two separate quotes (`> a` / blank / `> b`)', () => {
    const { prefix, tail } = expectWellFormed('> a\n\n> b')
    expect(prefix).toBe('> a\n\n')
    expect(tail).toBe('> b')
  })
})

describe('splitStreamingMarkdown — lists (loose items & continuations)', () => {
  it('does not split a loose list (`- a` / blank / `- b`)', () => {
    const list = '- a\n\n- b\n\n- c'
    const text = list + '\n\nAfter.'
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe(list + '\n\n')
    expect(tail).toBe('After.')
  })

  it('keeps an in-progress loose list wholly in the tail', () => {
    const { prefix, tail } = expectWellFormed('- a\n\n- b')
    expect(prefix).toBe('')
    expect(tail).toBe('- a\n\n- b')
  })

  it('does not split before an indented list-item continuation block', () => {
    const { prefix, tail } = expectWellFormed('1. item\n\n   continued paragraph')
    expect(prefix).toBe('')
    expect(tail).toBe('1. item\n\n   continued paragraph')
  })

  it('splits after a list ends (marker before, plain paragraph after)', () => {
    const { prefix, tail } = expectWellFormed('- a\n- b\n\nPlain text now.')
    expect(prefix).toBe('- a\n- b\n\n')
    expect(tail).toBe('Plain text now.')
  })

  it('splits before a list that starts fresh after a paragraph', () => {
    const { prefix, tail } = expectWellFormed('Intro text.\n\n- first item')
    expect(prefix).toBe('Intro text.\n\n')
    expect(tail).toBe('- first item')
  })
})

describe('splitStreamingMarkdown — indented code', () => {
  it('does not split the blank line inside one indented code block', () => {
    const code = '    code line 1\n\n    code line 2'
    const text = 'Lead.\n\n' + code
    const { prefix, tail } = expectWellFormed(text)
    expect(prefix).toBe('Lead.\n\n')
    expect(tail).toBe(code)
  })

  it('splits before a fresh indented code block', () => {
    const { prefix, tail } = expectWellFormed('Lead.\n\n    code block')
    expect(prefix).toBe('Lead.\n\n')
    expect(tail).toBe('    code block')
  })
})

describe('splitStreamingMarkdown — streaming simulation (append-only prefix)', () => {
  it('prefix only grows as chunks arrive, and reassembly is exact', () => {
    const full = [
      'First paragraph.',
      '',
      'Second paragraph with `code`.',
      '',
      '```ts',
      'const x = 1',
      '',
      'const y = 2',
      '```',
      '',
      'Closing paragraph.',
    ].join('\n')

    // Stream in 7-char chunks like the send engine's flushes do.
    let text = ''
    let prevPrefix = ''
    for (let end = 0; end <= full.length; end += 7) {
      text = full.slice(0, end)
      const { prefix } = expectWellFormed(text)
      // Append-stability: the finalized prefix never shrinks or rewrites.
      expect(prefix.startsWith(prevPrefix)).toBe(true)
      prevPrefix = prefix
    }
    // At completion the last paragraph is still tail; one more blank-line
    // terminator would finalize it — the finalized MessageBubble path owns
    // the final full-text render regardless.
    expect(prevPrefix).not.toBe('')
  })
})
