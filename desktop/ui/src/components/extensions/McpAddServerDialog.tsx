// McpAddServerDialog — modal hosting the three install flows for MCP servers.
//
// Tabs:
//   1. Search — debounced search over the MCP registry. One-click install
//      via buildSpecFromPackage + installMcpStdio (existing flow, moved here
//      verbatim).
//   2. Paste JSON — textarea accepting Cursor (`{ "mcpServers": { ... } }`)
//      or Claude Desktop / single-server shape. Parsed client-side, one
//      install_mcp_stdio call per server.
//   3. Manual — the legacy stdio form (name + command + args + env).
//
// Modal pattern mirrors InstallDialog.tsx: fixed overlay, Escape to close,
// click backdrop to close. The parent owns the installed list; we accept
// `onInstalled` and call it after every successful install so the page
// refreshes.

import { useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { Modal, ModalBody } from "@/components/ui/modal";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { SearchTab } from "./mcp-add-server-dialog/SearchTab";
import { PasteJsonTab } from "./mcp-add-server-dialog/PasteJsonTab";
import { ManualTab } from "./mcp-add-server-dialog/ManualTab";

export interface McpAddServerDialogProps {
  open: boolean;
  onClose: () => void;
  onInstalled: () => void;
  installedNames: Set<string>;
  /** Initial query seeded from the Extensions shell's shared search box. */
  initialQuery?: string;
}

// ===========================================================================
// Component
// ===========================================================================

type TabKey = "search" | "paste" | "manual";

export default function McpAddServerDialog({
  open,
  onClose,
  onInstalled,
  installedNames,
  initialQuery = "",
}: McpAddServerDialogProps) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [tab, setTab] = useState<TabKey>("search");

  // Reset to the first tab whenever the dialog is opened.
  useEffect(() => {
    if (open) setTab("search");
  }, [open]);

  // Modal owns the close-on-Escape + focus + scroll-lock + backdrop-click
  // primitives; we just supply the title/aria-label via props.

  const tabs: { key: TabKey; label: string }[] = [
    { key: "search", label: t("extensions.mcp.addDialog.tab.search") },
    { key: "paste", label: t("extensions.mcp.addDialog.tab.paste") },
    { key: "manual", label: t("extensions.mcp.addDialog.tab.manual") },
  ];

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="2xl"
      title={t("extensions.mcp.addDialog.title")}
      closeLabel={t("extensions.installDialog.closeAria")}
      className="max-w-3xl max-h-[90vh] overflow-y-auto"
    >
      <ModalBody className="flex flex-col gap-md">

        {/* Tab strip */}
        <div
          role="tablist"
          aria-label={t("extensions.mcp.addDialog.title")}
          className="flex gap-xs border-b border-outline-variant/30"
        >
          {tabs.map((tb) => (
            <Button
              key={tb.key}
              role="tab"
              type="button"
              variant="ghost"
              aria-selected={tab === tb.key}
              onClick={() => setTab(tb.key)}
              className={cn(
                "px-md py-sm rounded-none text-label-sm font-bold border-b-2 -mb-px transition-colors",
                tab === tb.key
                  ? "border-primary text-primary hover:bg-transparent"
                  : "border-transparent text-on-surface-variant hover:text-on-surface hover:bg-transparent",
              )}
            >
              {tb.label}
            </Button>
          ))}
        </div>

        {tab === "search" && (
          <SearchTab
            installedNames={installedNames}
            initialQuery={initialQuery}
            onInstalled={onInstalled}
          />
        )}
        {tab === "paste" && (
          <PasteJsonTab onInstalled={onInstalled} />
        )}
        {tab === "manual" && (
          <ManualTab onInstalled={onInstalled} />
        )}
      </ModalBody>
    </Modal>
  );
}