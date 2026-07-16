import hljs from 'highlight.js/lib/core'
import javascript from 'highlight.js/lib/languages/javascript'
import typescript from 'highlight.js/lib/languages/typescript'
import xml from 'highlight.js/lib/languages/xml'
import css from 'highlight.js/lib/languages/css'
import json from 'highlight.js/lib/languages/json'
import markdown from 'highlight.js/lib/languages/markdown'
import python from 'highlight.js/lib/languages/python'
import rust from 'highlight.js/lib/languages/rust'
import bash from 'highlight.js/lib/languages/bash'
import yaml from 'highlight.js/lib/languages/yaml'

hljs.registerLanguage('javascript', javascript)
hljs.registerLanguage('typescript', typescript)
hljs.registerLanguage('xml', xml)
hljs.registerLanguage('html', xml)
hljs.registerLanguage('css', css)
hljs.registerLanguage('json', json)
hljs.registerLanguage('markdown', markdown)
hljs.registerLanguage('python', python)
hljs.registerLanguage('rust', rust)
hljs.registerLanguage('bash', bash)
hljs.registerLanguage('shell', bash)
hljs.registerLanguage('yaml', yaml)
hljs.registerLanguage('yml', yaml)

export { hljs }

export const EXTENSION_TO_LANG: Record<string, string> = {
  ts: 'typescript', tsx: 'typescript', mts: 'typescript', cts: 'typescript',
  js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript',
  py: 'python',
  rs: 'rust',
  sh: 'bash', bash: 'bash', zsh: 'bash',
  yml: 'yaml', yaml: 'yaml',
  md: 'markdown', markdown: 'markdown',
  json: 'json',
  html: 'html', htm: 'html',
  xml: 'xml', svg: 'xml',
  css: 'css',
}

export const LANG_ALIASES: Record<string, string> = {
  ts: 'typescript', tsx: 'typescript',
  js: 'javascript', jsx: 'javascript',
  py: 'python',
  rs: 'rust',
  sh: 'bash', shell: 'bash',
  yml: 'yaml',
  md: 'markdown',
}

export function resolveLanguage(language: string | undefined, fileName: string | undefined): string | undefined {
  if (language) {
    const lower = language.toLowerCase()
    const norm = LANG_ALIASES[lower] ?? lower
    if (hljs.getLanguage(norm)) return norm
    if (lower === 'text' || lower === 'plaintext' || lower === 'plain') return undefined
  }
  if (fileName) {
    const ext = fileName.split('.').pop()?.toLowerCase() ?? ''
    const lang = EXTENSION_TO_LANG[ext]
    if (lang && hljs.getLanguage(lang)) return lang
  }
  return undefined
}
