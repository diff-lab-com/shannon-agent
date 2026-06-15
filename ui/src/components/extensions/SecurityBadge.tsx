import { useEffect, useState } from "react";
import {
  scanPromptInjection,
  type InjectionRisk,
} from "@/lib/tauri-api";

/**
 * P6 Security badge — runs an async prompt-injection scan over the entry's
 * description and shows a warning chip when the risk is non-clean.
 *
 * Used inside catalog cards (Skills, Agents, Data Sources). Only the
 * `description` is scanned for the MVP — full README scanning is a follow-up.
 *
 * The scan runs lazily: when this component mounts, it kicks off one invoke
 * and caches the result in local state. Re-renders don't re-scan.
 */
export function SecurityBadge({
  text,
  trust,
}: {
  text: string
  trust: "verified" | "official" | "community" | "unknown"
}) {
  // Verified/official entries skip the scan — Shannon already trusts them.
  const skip = trust === "verified" || trust === "official"
  const [risk, setRisk] = useState<InjectionRisk | null>(null)

  useEffect(() => {
    if (skip) return
    let cancelled = false
    scanPromptInjection(text)
      .then((report) => {
        if (!cancelled) setRisk(report.risk)
      })
      .catch(() => {
        // Silent — scan failure shouldn't break the card render.
      })
    return () => {
      cancelled = true
    }
  }, [text, skip])

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
