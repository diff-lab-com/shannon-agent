import { useOutletContext } from "react-router-dom";

/**
 * P1 placeholder for the MCP Servers tab.
 *
 * P2 will wire this to the MCP Registry fetcher + OAuth installer. P1 ships
 * empty state pointing the user to the existing manual MCP config in Settings.
 */
export default function McpServers() {
  const { search } = useOutletContext<{ search: string }>();

  return (
    <div className="p-lg max-w-4xl mx-auto">
      <div className="text-center py-3xl">
        <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-primary/10 mb-md">
          <span className="material-symbols-outlined text-primary text-[32px]">cloud</span>
        </div>
        <h2 className="text-headline-md font-bold text-on-surface mb-sm">
          MCP Servers
        </h2>
        <p className="text-body-md text-on-surface-variant max-w-md mx-auto">
          Browse the <a
            href="https://registry.modelcontextprotocol.io"
            target="_blank"
            rel="noreferrer"
            className="text-primary underline"
          >MCP Registry</a> and connect vendor-hosted servers with one click.
          Coming in P2.
        </p>
        {search && (
          <p className="text-label-sm text-outline mt-md">
            (search "{search}" — catalog not yet available)
          </p>
        )}
      </div>

      <div className="border border-outline-variant/30 rounded-2xl p-lg bg-surface-container-low/50">
        <div className="flex items-start gap-md">
          <span className="material-symbols-outlined text-primary text-[24px] mt-xs">lightbulb</span>
          <div className="flex-1">
            <h3 className="font-bold text-label-md text-on-surface mb-xs">
              Want to add an MCP server now?
            </h3>
            <p className="text-label-sm text-on-surface-variant mb-md">
              The Settings page still supports manual MCP server configuration
              via stdio. Use it for any server the registry doesn't cover.
            </p>
            <a
              href="/settings"
              className="inline-flex items-center gap-xs text-label-sm text-primary font-bold hover:underline"
            >
              <span className="material-symbols-outlined text-[16px]">settings</span>
              Open Settings
            </a>
          </div>
        </div>
      </div>
    </div>
  );
}
