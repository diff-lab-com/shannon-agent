import { describe, expect, it } from 'vitest'
import { buildSrcDoc } from '../MermaidRenderer'

/**
 * Review §P3 (桌面): the diagram source is inlined into a <script> block of
 * the iframe srcDoc. A raw `</script>` in the source (whether from a
 * malicious or an accidental diagram) would close the script element early
 * and inject markup into the srcdoc document.
 */
describe('buildSrcDoc script-block escaping', () => {
  it('escapes </script> in the diagram source', () => {
    const src = buildSrcDoc(
      'graph TD\n  A["</script><img src=x onerror=alert(1)>"] --> B',
      'Loading…',
      'Failed',
      'light',
    )
    // The raw closing tag must never appear inside the injected payload…
    expect(src).not.toContain('</script><img')
    // …only the escaped form, which parses back to "<" inside the script…
    expect(src).toContain('\\u003c/script>\\u003cimg')
    // …and the real document structure stays intact: exactly one real
    // script close — the document's own, after the body.
    expect(src.match(/<\/script>/g)).toHaveLength(1)
    expect(src.trimEnd().endsWith('</html>')).toBe(true)
  })

  it('escapes every < so no markup can be injected from the source', () => {
    const src = buildSrcDoc('<b>bold</b> <img src=x>', 'Loading…', 'Failed', 'dark')
    // Only "<" is escaped — ">", "/" and text stay literal.
    expect(src).toContain('\\u003cb>bold\\u003c/b> \\u003cimg src=x>')
    expect(src).not.toContain('<b>bold')
  })

  it('keeps a benign diagram byte-identical apart from escaping', () => {
    const src = buildSrcDoc('graph TD; A --> B;', 'Loading…', 'Failed', 'light')
    expect(src).toContain("const source = \"graph TD; A --> B;\";")
  })

  it('still embeds theme mode and labels', () => {
    const src = buildSrcDoc('graph TD; A --> B;', '载入中', '渲染失败', 'dark')
    expect(src).toContain("data-mode=\"dark\"")
    expect(src).toContain('载入中')
    expect(src).toContain("'dark'")
  })
})
