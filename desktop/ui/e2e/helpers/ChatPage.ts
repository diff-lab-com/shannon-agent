// ChatPage page object (R1 chat-testing infra).
//
// Locators anchor ONLY on stable attributes — role + aria-label / data-testid
// — never on placeholder or translated prose that shifts with i18n. The aria
// labels below are the en-locale strings (Playwright contexts default to
// en-US): composer "Message", send "Send message", stop "Stop generation"
// (src/i18n/locales/en.json; ChatInput.tsx wires them).
//
//   composer        role=textbox  aria-label="Message"          (ChatInput.tsx:1105)
//   send button     role=button   aria-label="Send message"     (ChatInput.tsx:1436)
//   stop button     role=button   aria-label="Stop generation"  (ChatInput.tsx:1427)
//   streaming text  .streaming-cursor                           (StreamingResponse.tsx:64)
//   bubbles         [data-message-index=<n>]                    (MessageArea.tsx:305)
import { expect, type Locator, type Page } from '@playwright/test'

export class ChatPage {
  readonly page: Page

  constructor(page: Page) {
    this.page = page
  }

  /** The composer textbox (present in both welcome and conversation states). */
  composer(): Locator {
    return this.page.getByRole('textbox', { name: 'Message' })
  }

  sendButton(): Locator {
    return this.page.getByRole('button', { name: 'Send message' })
  }

  /** Visible only while a run is in flight (swaps into the send slot). */
  stopButton(): Locator {
    return this.page.getByRole('button', { name: 'Stop generation' })
  }

  streamingCursor(): Locator {
    return this.page.locator('.streaming-cursor')
  }

  /** All committed bubbles (user + assistant), in DOM order. */
  bubbles(): Locator {
    return this.page.locator('[data-message-index]')
  }

  bubbleAt(index: number): Locator {
    // Attribute value MUST be quoted — querySelectorAll rejects bare numbers
    // in CSS attribute selectors ([data-message-index=1] is invalid CSS).
    return this.page.locator(`[data-message-index="${index}"]`)
  }

  /**
   * Jump the message area to its bottom. The list VIRTUALIZES past 30
   * messages (react-virtual): rows outside the viewport window are not in
   * the DOM, so a test that asserts a specific tail row must scroll the
   * scroll container to it FIRST (the initial auto-follow is smooth and can
   * still be mid-flight — or interrupted — on a loaded runner).
   */
  async scrollToBottom(): Promise<void> {
    await this.page.evaluate(() => {
      const el = document.querySelector<HTMLElement>('[data-testid="chat-scroll-container"]')
      if (!el) throw new Error('chat scroll container not found — MessageArea testid missing?')
      el.scrollTop = el.scrollHeight
    })
  }

  runStatusLine(): Locator {
    return this.page.getByTestId('run-status-line')
  }

  async send(text: string): Promise<void> {
    await this.composer().fill(text)
    // Enter sends (ChatInput handleKeyDown); the button click would queue
    // instead when a run is already streaming.
    await this.composer().press('Enter')
  }

  async stop(): Promise<void> {
    await this.stopButton().click()
  }

  async expectStreamingCursor(): Promise<void> {
    await expect(this.streamingCursor()).toBeVisible()
  }

  /**
   * Assert the bubble at `index` carries `text`, whitespace-normalized —
   * Markdown renders block elements (no stable raw newlines) and the bubble
   * frame carries chrome (avatar label, timestamp, actions), so text is
   * extracted from the `.prose` content container when present (assistant
   * bubbles; user bubbles fall back to the whole bubble text).
   */
  async expectBubbleText(index: number, text: string): Promise<void> {
    const bubble = this.bubbleAt(index)
    const prose = bubble.locator('.prose').first()
    const raw = (await prose.count()) > 0 ? await prose.textContent() : await bubble.textContent()
    expect((raw ?? '').replace(/\s+/g, ' ').trim()).toBe(text.replace(/\s+/g, ' ').trim())
  }

  async messageCount(): Promise<number> {
    return this.bubbles().count()
  }

  async isStopVisible(): Promise<boolean> {
    return this.stopButton().isVisible()
  }
}
