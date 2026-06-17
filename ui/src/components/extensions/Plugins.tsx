import { FormattedMessage, useIntl } from "react-intl";
import { useOutletContext } from "react-router-dom";

const PLACEHOLDER_REPOS = [
  { repo: "anthropics/skills", entries: 149, descKey: "extensions.plugins.anthropics.description" },
  { repo: "ComposioHQ/awesome-claude-skills", entries: 1000, descKey: "extensions.plugins.composio.description" },
  { repo: "obra/superpowers", entries: 20, descKey: "extensions.plugins.obra.description" },
  { repo: "VoltAgent/awesome-claude-code-subagents", entries: 100, descKey: "extensions.plugins.voltagent.description" },
];

export default function Plugins() {
  const intl = useIntl();
  const { search } = useOutletContext<{ search: string }>();

  return (
    <div className="p-lg max-w-4xl mx-auto">
      <div className="text-center py-3xl">
        <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-primary/10 mb-md">
          <span className="material-symbols-outlined text-primary text-[32px]">workspaces</span>
        </div>
        <h2 className="text-headline-md font-bold text-on-surface mb-sm">
          {intl.formatMessage({ id: "extensions.plugins.title" })}
        </h2>
        <p className="text-body-md text-on-surface-variant max-w-md mx-auto">
          <FormattedMessage
            id="extensions.plugins.description"
            values={{
              code: (chunks) => (
                <code className="mx-xs px-xs py-[1px] bg-surface-container-low rounded font-mono text-label-sm">
                  {chunks}
                </code>
              ),
            }}
          />
        </p>
        {search && (
          <p className="text-label-sm text-outline mt-md">
            <FormattedMessage id="extensions.plugins.searchPreview" values={{ query: search }} />
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
                <p className="text-label-xs text-on-surface-variant">
                  <FormattedMessage id="extensions.plugins.entries" values={{ count: repo.entries }} />
                </p>
              </div>
              <span className="text-label-xs px-sm py-[2px] rounded-full bg-tertiary-container/50 text-on-tertiary-container font-bold">
                {intl.formatMessage({ id: "extensions.plugins.comingSoon" })}
              </span>
            </div>
            <p className="text-label-sm text-on-surface-variant">{intl.formatMessage({ id: repo.descKey })}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
