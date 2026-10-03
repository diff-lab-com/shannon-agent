// Tauri v2 webview drag-drop seam (journey #9 queue-steer, journey #10
// edit-rewind A-26).
//
// Delivers a Tauri v2 webview drag-drop through the mock event bridge — the
// demo-mode path into the composer's mergePaths (the native attach dialog is
// not drivable in the harness). The real @tauri-apps/api webview listener
// registers `tauri://drag-drop` through plugin:event|listen, which the mock
// bridge fans out to.
import type { Page } from '@playwright/test'

export async function emitWebviewDrop(page: Page, paths: string[]): Promise<void> {
  await page.evaluate((dropped) => {
    ;(window as unknown as {
      __shannonMock: { emit(name: string, payload: unknown): void }
    }).__shannonMock.emit('tauri://drag-drop', { paths: dropped, position: { x: 0, y: 0 } })
  }, paths)
}
