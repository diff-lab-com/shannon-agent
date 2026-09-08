// Tab 3: Manual stdio form
//
// Legacy stdio form (name + command + args + env). Kept as-is from the prior
// McpServers.tsx implementation.

import { useState } from "react";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import {
  installMcpStdio,
  type StdioMcpSpecPayload,
} from "@/lib/tauri-api";
import { safeErrorMessage } from "@/lib/packageValidation";
import { Button } from "@/components/ui/button";
import { parseArgs, parseEnv } from "./utils";

export function ManualTab({ onInstalled }: { onInstalled: () => void }) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [envText, setEnvText] = useState("");
  const [busy, setBusy] = useState(false);

  async function handleSubmit() {
    if (!name.trim() || !command.trim()) {
      toast.error(t("extensions.mcp.needNameAndCommand"));
      return;
    }
    setBusy(true);
    try {
      const spec: StdioMcpSpecPayload = {
        server_name: name.trim(),
        command: command.trim(),
        args: parseArgs(argsText),
        env: parseEnv(envText),
      };
      await installMcpStdio(spec);
      toast.success(
        t("extensions.mcp.oneClick.installSuccess", { name: name.trim() }),
      );
      onInstalled();
      setName("");
      setCommand("");
      setArgsText("");
      setEnvText("");
    } catch (err) {
      toast.error(
        t("extensions.mcp.oneClick.installFailed", {
          error: safeErrorMessage(err, "install failed"),
        }),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-sm"
      role="tabpanel"
      aria-label={t("extensions.mcp.addDialog.manual.aria")}
    >
      <p className="text-label-sm text-on-surface-variant">
        {t("extensions.mcp.manualDesc")}
      </p>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.serverNameRequired")}
        </span>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('extensions.mcp.serverName.placeholder')}
          className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface"
          disabled={busy}
        />
      </label>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.commandRequired")}
        </span>
        <input
          type="text"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          placeholder="npx"
          className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
          disabled={busy}
        />
      </label>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.argsLabel")}
        </span>
        <input
          type="text"
          value={argsText}
          onChange={(e) => setArgsText(e.target.value)}
          placeholder="-y @modelcontextprotocol/server-filesystem /tmp"
          className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
          disabled={busy}
        />
      </label>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.envLabel")}
        </span>
        <textarea
          value={envText}
          onChange={(e) => setEnvText(e.target.value)}
          rows={3}
          placeholder={"ROOT=/tmp\nLOG_LEVEL=info"}
          className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
          disabled={busy}
        />
      </label>
      <Button
        type="button"
        onClick={handleSubmit}
        disabled={busy}
        className="px-md py-sm rounded-lg hover:bg-primary/90 cursor-pointer"
      >
        <span className="material-symbols-outlined icon-sm">
          {busy ? "progress_activity" : "add"}
        </span>
        {busy
          ? t("extensions.mcp.installing")
          : t("extensions.mcp.install")}
      </Button>
    </div>
  );
}