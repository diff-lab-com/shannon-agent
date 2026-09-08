/// <reference types="vite/client" />

declare const __APP_VERSION__: string

declare module '@fontsource-variable/inter' {
  const css: string
  export default css
}

declare module '@fontsource-variable/material-symbols-outlined/full.css' {
  const css: string
  export default css
}

// P1-5 D — xterm stylesheet for the integrated terminal (side-effect import
// in components/terminal/TerminalPanel.tsx; Vite inlines it at build time).
declare module '@xterm/xterm/css/xterm.css' {
  const css: string
  export default css
}
