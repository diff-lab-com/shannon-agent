// Tab 2: Paste JSON
//
// Textarea accepting Cursor (`{ "mcpServers": { ... } }`) or Claude Desktop /
// single-server shape. Parsed client-side, one install_mcp_stdio call per
// server.

import { useState } from "react";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import { installMcpStdio } from "@/lib/tauri-api";
import { safeErrorMessage } from "@/lib/packageValidation";
import { Button } from "@/components/ui/button";
import { parseMcpJson } from "./utils";
import type { ParsedMcpServer } from "./types";

export function PasteJsonTab({ onInstalled }: { onInstalled: () => void }) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [text, setText] = useState("");
  const [parsed, setParsed] = useState<ParsedMcpServer[] | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  function handleParse(raw: string) {
    setText(raw);
    if (!raw.trim()) {
      setParsed(null);
      setParseError(null);
      return;
    }
    try {
      const servers = parseMcpJson(raw);
      setParsed(servers);
      setParseError(null);
    } catch (e) {
      setParsed(null);
      setParseError(safeErrorMessage(e, "parse failed"));
    }
  }

  async function handleInstallAll() {
    if (!parsed || parsed.length === 0) return;
    setInstalling(true);
    let ok = 0;
    let failed = 0;
    for (const srv of parsed) {
      try {
        await installMcpStdio({
          server_name: srv.name,
          command: srv.command,
          args: srv.args,
          env: srv.env,
        });
        ok++;
      } catch {
        failed++;
      }
    }
    setInstalling(false);
    if (ok > 0) {
      toast.success(
        intl.formatMessage(
          { id: "extensions.mcp.oneClick.installSuccessCount" },
          { count: ok },
        ),
      );
      onInstalled();
    }
    if (failed > 0) {
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.oneClick.installFailedCount" },
          { count: failed },
        ),
      );
    }
  }

  return (
    <div className="flex flex-col gap-sm" role="tabpanel">
      <textarea
        value={text}
        onChange={(e) => handleParse(e.target.value)}
        placeholder={t("extensions.mcp.addDialog.paste.placeholder")}
        aria-label={t("extensions.mcp.addDialog.paste.placeholder")}
        rows={10}
        className="w-full px-md py-sm rounded-lg border border-outline-variant/40 bg-surface text-label-sm font-mono focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        spellCheck={false}
      />

      {parseError && (
        <div className="border border-error/30 rounded-xl p-sm bg-error-container/10 text-label-xs text-error">
          {t("extensions.mcp.addDialog.paste.parseError", {
            error: parseError,
          })}
        </div>
      )}

      {parsed && parsed.length > 0 && (
        <div className="flex items-center justify-between gap-sm">
          <ul className="flex-1 min-w-0 text-label-xs text-on-surface-variant list-disc pl-md">
            {parsed.map((s, i) => (
              <li key={`${s.name}-${i}`} className="truncate">
                <span className="font-mono">{s.name}</span> —{" "}
                <span className="font-mono">{s.command}</span>
              </li>
            ))}
          </ul>
          <Button
            type="button"
            onClick={handleInstallAll}
            disabled={installing}
            className="shrink-0 px-md py-sm rounded-lg hover:bg-primary/90 cursor-pointer"
          >
            <span className="material-symbols-outlined icon-sm">
              {installing ? "progress_activity" : "download"}
            </span>
            {t("extensions.mcp.addDialog.paste.install", { count: parsed.length })}
          </Button>
        </div>
      )}
    </div>
  );
}