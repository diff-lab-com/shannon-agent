/// <reference types="vite/client" />

declare module '@fontsource-variable/geist' {
  const css: string
  export default css
}

declare module '@fontsource-variable/inter' {
  const css: string
  export default css
}

declare module '@fontsource-variable/material-symbols-outlined/full.css' {
  const css: string
  export default css
}

interface Window {
  __TAURI__?: unknown
}
