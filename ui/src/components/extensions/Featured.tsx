import { useOutletContext } from "react-router-dom";

/**
 * P1 placeholder for the Featured tab.
 *
 * Will be replaced in P2 with curated vendor cards (Notion, Linear, Slack,
 * GitHub, Gmail-OAuth) + popular skill/agent picks. P1 ships empty state so
 * the tab structure is in place.
 */
export default function Featured() {
  const { search } = useOutletContext<{ search: string }>();

  return (
    <div className="p-lg max-w-5xl mx-auto">
      <div className="text-center py-3xl">
        <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-primary/10 mb-md">
          <span className="material-symbols-outlined text-primary text-[32px]">auto_awesome</span>
        </div>
        <h2 className="text-headline-md font-bold text-on-surface mb-sm">
          Featured Extensions
        </h2>
        <p className="text-body-md text-on-surface-variant max-w-md mx-auto">
          Curated MCP servers, skills, and agents — verified by Shannon.
          Coming in P2 with the MCP Registry and featured vendor list.
        </p>
        {search && (
          <p className="text-label-sm text-outline mt-md">
            (search "{search}" — catalog not yet available)
          </p>
        )}
      </div>

      {/* Placeholder grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-md mt-xl">
        {PLACEHOLDER_FEATURED.map((entry) => (
          <div
            key={entry.id}
            className="border border-outline-variant/30 rounded-2xl p-lg bg-surface-container-low/50 opacity-60"
          >
            <div className="flex items-start justify-between mb-sm">
              <div className="w-10 h-10 rounded-lg bg-primary/10 flex items-center justify-center">
                <span className="material-symbols-outlined text-primary text-[20px]">{entry.icon}</span>
              </div>
              <span className="text-label-xs px-sm py-[2px] rounded-full bg-tertiary-container/50 text-on-tertiary-container font-bold">
                Soon
              </span>
            </div>
            <h3 className="font-bold text-label-md text-on-surface mb-xs">{entry.name}</h3>
            <p className="text-label-sm text-on-surface-variant">{entry.description}</p>
          </div>
        ))}
      </div>
    </div>
  );
}

const PLACEHOLDER_FEATURED = [
  {
    id: "notion",
    icon: "description",
    name: "Notion",
    description: "OAuth-hosted remote MCP server. Read/write pages, databases, comments.",
  },
  {
    id: "linear",
    icon: "view_kanban",
    name: "Linear",
    description: "OAuth-hosted remote MCP server. Issues, projects, sprints.",
  },
  {
    id: "slack",
    icon: "tag",
    name: "Slack",
    description: "OAuth-hosted remote MCP server. Channel messages, threads, files.",
  },
  {
    id: "github",
    icon: "code",
    name: "GitHub",
    description: "OAuth-hosted remote MCP server. Repos, issues, PRs, code search.",
  },
  {
    id: "gmail",
    icon: "mail",
    name: "Gmail",
    description: "OAuth-hosted remote MCP server. Read, send, search email.",
  },
  {
    id: "obsidian",
    icon: "menu_book",
    name: "Obsidian Vault",
    description: "Native Rust tool. Pick a vault directory, grant FS access.",
  },
];
