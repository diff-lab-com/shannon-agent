import { useOutletContext } from "react-router-dom";

/**
 * P1 placeholder for the Plugins tab.
 *
 * Plugins are bundles — `.claude-plugin/marketplace.json` repos that install
 * multiple skills/agents/MCP configs at once. The installer lands in P3
 * (MarketplacePluginInstaller); this tab shows empty state until then.
 */
export default function Plugins() {
  const { search } = useOutletContext<{ search: string }>();

  return (
    <div className="p-lg max-w-4xl mx-auto">
      <div className="text-center py-3xl">
        <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-primary/10 mb-md">
          <span className="material-symbols-outlined text-primary text-[32px]">workspaces</span>
        </div>
        <h2 className="text-headline-md font-bold text-on-surface mb-sm">
          Plugins
        </h2>
        <p className="text-body-md text-on-surface-variant max-w-md mx-auto">
          Install a bundle of skills, agents, and MCP servers from a single
          <code className="mx-xs px-xs py-[1px] bg-surface-container-low rounded font-mono text-label-sm">.claude-plugin/marketplace.json</code>
          repo. Coming in P3.
        </p>
        {search && (
          <p className="text-label-sm text-outline mt-md">
            (search "{search}" — catalog not yet available)
          </p>
        )}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md mt-xl">
        {PLACEHOLDER_REPOS.map((repo) => (
          <div
            key={repo.repo}
            className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/50 opacity-60"
          >
            <div className="flex items-start justify-between mb-xs">
              <div>
                <h3 className="font-bold text-label-md text-on-surface">{repo.repo}</h3>
                <p className="text-label-xs text-on-surface-variant">{repo.entries} entries</p>
              </div>
              <span className="text-label-xs px-sm py-[2px] rounded-full bg-tertiary-container/50 text-on-tertiary-container font-bold">
                Soon
              </span>
            </div>
            <p className="text-label-sm text-on-surface-variant">{repo.description}</p>
          </div>
        ))}
      </div>
    </div>
  );
}

const PLACEHOLDER_REPOS = [
  {
    repo: "anthropics/skills",
    entries: 149,
    description: "Official Anthropic skill examples (Apache-2.0).",
  },
  {
    repo: "ComposioHQ/awesome-claude-skills",
    entries: 1000,
    description: "Community skill bundle (~1000 SKILL.md files).",
  },
  {
    repo: "obra/superpowers",
    entries: 20,
    description: "High-quality workflow skills (MIT).",
  },
  {
    repo: "VoltAgent/awesome-claude-code-subagents",
    entries: 100,
    description: "Subagent definitions in Claude Code format.",
  },
];
