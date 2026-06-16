import { useEffect, useState } from "react";
import {
  scanPromptInjectionWithReadme,
  type InjectionRisk,
} from "@/lib/tauri-api";

/**
 * P6 Security badge — runs an async prompt-injection scan over the entry's
 * description (and README body when `readmeUrl` is provided), then shows a
 * warning chip when the risk is non-clean.
 *
 * D1: when `readmeUrl` is supplied, the backend fetches the README (10s
 * timeout, 32KB truncation, 24h in-memory cache) and combines it with the
 * description before scanning. Fetch failures fall back to description-only.
 *
 * Verified/official entries skip the scan — Shannon already trusts them.
 */
export function SecurityBadge({
  text,
  trust,
  readmeUrl,
}: {
  text: string
  trust: "verified" | "official" | "community" | "unknown"
  /**
   * Optional README URL. When provided, the backend augments the scan with
   * the README body. Leave undefined to scan description only (legacy mode).
   */
  readmeUrl?: string
}) {
  const skip = trust === "verified" || trust === "official"
  const [risk, setRisk] = useState<InjectionRisk | null>(null)

  useEffect(() => {
    if (skip) return
    let cancelled = false
    const url = readmeUrl && readmeUrl.trim() ? readmeUrl.trim() : null
    scanPromptInjectionWithReadme(text, url)
      .then((report) => {
        if (!cancelled) setRisk(report.risk)
      })
      .catch(() => {
        // Silent — scan failure shouldn't break the card render.
      })
    return () => {
      cancelled = true
    }
  }, [text, skip, readmeUrl])

  if (skip || risk === null || risk === "clean") return null

  if (risk === "dangerous") {
    return (
      <span
        className="text-label-xs px-xs py-[1px] rounded-full font-bold bg-error-container/60 text-on-error-container flex items-center gap-[2px]"
        title="Prompt-injection patterns detected. Review before install."
      >
        <span className="material-symbols-outlined text-[12px]">warning</span>
        Injection risk
      </span>
    )
  }

  return (
    <span
      className="text-label-xs px-xs py-[1px] rounded-full font-bold bg-tertiary-container/60 text-on-tertiary-container flex items-center gap-[2px]"
      title="Possible prompt-injection patterns detected. Review the description."
    >
      <span className="material-symbols-outlined text-[12px]">info</span>
      Review
    </span>
  )
}
