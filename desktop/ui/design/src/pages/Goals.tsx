import React from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export default function Goals() {
  return (
    <div className="flex-1 flex w-full h-full pb-10">
      {/* Sidebar for Goals */}
      <aside className="w-[320px] h-full border-r border-outline-variant/20 bg-surface-container-low/30 flex flex-col overflow-hidden shrink-0">
        <div className="p-md border-b border-outline-variant/20">
          <div className="relative">
            <span className="material-symbols-outlined absolute left-3 top-1/2 -translate-y-1/2 text-on-surface-variant/60 text-[20px]">
              search
            </span>
            <Input
              className="w-full pl-10 pr-4 py-2 bg-surface-container-lowest border border-outline-variant/50 rounded-lg text-body-sm focus:outline-none focus:border-primary transition-all outline-none"
              placeholder="Search goals..."
              type="text"
            />
          </div>
        </div>
        <div className="flex-1 overflow-y-auto py-sm">
          <div className="px-md py-xs">
            <p className="font-label-sm text-on-surface-variant/60 uppercase tracking-wider">
              Active Goals
            </p>
          </div>
          <div className="px-sm space-y-1">
            <Button className="w-full flex flex-col gap-1 p-md rounded-xl bg-primary/10 border border-primary/20 text-left cursor-pointer">
              <div className="flex justify-between items-start">
                <span className="font-label-md text-primary font-bold">
                  Launch a New Product Campaign
                </span>
                <span className="font-label-sm text-primary">34%</span>
              </div>
              <div className="w-full h-1 bg-primary/20 rounded-full overflow-hidden">
                <div className="h-full bg-primary w-[34%]"></div>
              </div>
            </Button>
            <Button className="w-full flex flex-col gap-1 p-md rounded-xl hover:bg-surface-container-high/60 hover:shadow-sm hover:-translate-y-0.5 transition-all text-left cursor-pointer duration-300">
              <div className="flex justify-between items-start">
                <span className="font-label-md text-on-surface">
                  Q3 Financial Audit
                </span>
                <span className="font-label-sm text-on-surface-variant">
                  82%
                </span>
              </div>
              <div className="w-full h-1 bg-surface-container-highest rounded-full overflow-hidden">
                <div className="h-full bg-green-500 w-[82%]"></div>
              </div>
            </Button>
            <Button className="w-full flex flex-col gap-1 p-md rounded-xl hover:bg-surface-container-high/60 hover:shadow-sm hover:-translate-y-0.5 transition-all text-left cursor-pointer duration-300">
              <div className="flex justify-between items-start">
                <span className="font-label-md text-on-surface">
                  Customer Support Automation
                </span>
                <span className="font-label-sm text-on-surface-variant">
                  15%
                </span>
              </div>
              <div className="w-full h-1 bg-surface-container-highest rounded-full overflow-hidden">
                <div className="h-full bg-primary w-[15%]"></div>
              </div>
            </Button>
          </div>
        </div>
      </aside>

      {/* Goal Details Main Canvas */}
      <div className="flex-1 flex flex-col overflow-y-auto p-xl relative">
        <div className="flex items-end justify-between mb-xl">
          <div>
            <div className="flex items-center gap-sm mb-xs">
              <span className="px-sm py-xs bg-primary/10 text-primary font-label-sm rounded-full">
                Active Campaign
              </span>
              <span className="font-label-sm text-on-surface-variant/60">
                Started 2 days ago
              </span>
            </div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface">
              Launch a New Product Campaign
            </h2>
          </div>
          <div className="flex items-center gap-md">
            <div className="text-right">
              <p className="font-label-sm text-on-surface-variant">
                Completion
              </p>
              <p className="font-headline-md text-primary">34%</p>
            </div>
            <div className="w-32 h-2 bg-surface-container-high rounded-full overflow-hidden">
              <div className="h-full bg-primary w-[34%]"></div>
            </div>
          </div>
        </div>

        {/* Goal Tree Visualization */}
        <div className="flex gap-gutter w-full">
          {/* Agent Call Path */}
          <div className="w-1/4 max-w-[280px]">
            <div className="glass-card bg-white/70 p-md rounded-xl sticky top-0">
              <h3 className="font-label-md text-on-surface mb-md flex items-center gap-sm">
                <span className="material-symbols-outlined text-primary text-[20px]">
                  hub
                </span>{" "}
                Agent Call Path
              </h3>
              <div className="space-y-lg relative">
                {/* Visual Connectors */}
                <div className="absolute left-[15px] top-6 bottom-6 w-px border-l border-dashed border-primary/30"></div>
                {/* Path Items */}
                <div className="relative flex items-center gap-md">
                  <div className="z-10 w-8 h-8 rounded-full bg-primary text-on-primary flex items-center justify-center text-[18px]">
                    <span className="material-symbols-outlined">search</span>
                  </div>
                  <div>
                    <p className="font-label-md text-on-surface">Researcher</p>
                    <p className="font-label-sm text-on-surface-variant/70">
                      Analysis Bot
                    </p>
                  </div>
                </div>
                <div className="relative flex items-center gap-md">
                  <div className="z-10 w-8 h-8 rounded-full bg-primary-container text-on-primary-container flex items-center justify-center text-[18px]">
                    <span className="material-symbols-outlined">
                      edit_note
                    </span>
                  </div>
                  <div>
                    <p className="font-label-md text-on-surface">Copywriter</p>
                    <p className="font-label-sm text-on-surface-variant/70">
                      Drafting Logic
                    </p>
                  </div>
                </div>
                <div className="relative flex items-center gap-md opacity-40">
                  <div className="z-10 w-8 h-8 rounded-full bg-surface-container-highest text-on-surface flex items-center justify-center text-[18px]">
                    <span className="material-symbols-outlined">schedule</span>
                  </div>
                  <div>
                    <p className="font-label-md text-on-surface">Scheduler</p>
                    <p className="font-label-sm text-on-surface-variant/70">
                      Wait-state
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* Task Decomposition Tree */}
          <div className="flex-1 space-y-md">
            {/* Done Node */}
            <div className="flex items-start gap-lg">
              <div className="mt-4 flex flex-col items-center">
                <div className="w-4 h-4 rounded-full border-2 border-primary bg-background shadow-sm z-10"></div>
                <div className="w-px h-24 node-connector"></div>
              </div>
              <div className="flex-1 glass-card bg-white/70 p-lg rounded-xl flex justify-between items-center group hover:shadow-md transition-all">
                <div>
                  <div className="flex items-center gap-md mb-xs">
                    <h4 className="font-headline-md text-on-surface">
                      Market Analysis
                    </h4>
                    <span className="px-sm py-xs bg-green-100 text-green-700 font-label-sm rounded-lg flex items-center gap-1">
                      <span className="material-symbols-outlined text-[14px]">
                        check_circle
                      </span>{" "}
                      Done
                    </span>
                  </div>
                  <p className="text-on-surface-variant max-w-lg">
                    Comprehensive review of competitor landscape, pricing
                    strategies, and audience segmentation for the Q4 launch.
                  </p>
                </div>
                <Button className="p-sm text-on-surface-variant opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer">
                  <span className="material-symbols-outlined">more_vert</span>
                </Button>
              </div>
            </div>

            {/* Active Node with Human in the loop */}
            <div className="flex items-start gap-lg">
              <div className="mt-4 flex flex-col items-center">
                <div className="w-4 h-4 rounded-full border-2 border-primary bg-primary shadow-lg z-10"></div>
                <div className="w-px h-24 node-connector"></div>
              </div>
              <div className="flex-1 glass-card bg-white p-lg rounded-xl border-primary/30 ring-1 ring-primary/10 shadow-lg relative overflow-hidden">
                <div className="absolute top-0 right-0 p-lg">
                  <div className="animate-pulse-amber w-3 h-3 rounded-full bg-tertiary shadow-lg"></div>
                </div>
                <div className="mb-lg">
                  <div className="flex items-center gap-md mb-xs">
                    <h4 className="font-headline-md text-primary">
                      Draft Marketing Copy
                    </h4>
                    <span className="px-sm py-xs bg-primary/10 text-primary font-label-sm rounded-lg flex items-center gap-1">
                      <span className="material-symbols-outlined text-[14px]">
                        sync
                      </span>{" "}
                      In Progress
                    </span>
                  </div>
                  <p className="text-on-surface-variant">
                    Generating 12 variations of ad copy and email sequences
                    based on Market Analysis insights.
                  </p>
                </div>

                {/* Steps */}
                <div className="bg-surface-container-low/50 rounded-lg p-md space-y-md">
                  <p className="font-label-sm text-primary uppercase tracking-wider mb-sm">
                    Agent Reasoning Steps
                  </p>
                  <div className="flex items-start gap-md">
                    <div className="mt-1 flex flex-col items-center">
                      <div className="w-2 h-2 rounded-full bg-primary"></div>
                      <div className="w-px h-8 bg-outline-variant/50"></div>
                    </div>
                    <span className="font-label-sm text-on-surface-variant">
                      Identifying key value propositions from competitor data...
                    </span>
                  </div>
                  <div className="flex items-start gap-md">
                    <div className="mt-1 flex flex-col items-center">
                      <div className="w-2 h-2 rounded-full bg-primary animate-pulse"></div>
                    </div>
                    <div className="flex-1">
                      <span className="font-label-sm text-on-surface font-bold">
                        Awaiting verification of brand tone intensity.
                      </span>
                      <div className="mt-md flex gap-sm">
                        <Button className="px-md py-sm bg-tertiary text-on-tertiary rounded-lg font-label-md shadow-sm hover:brightness-110 active:scale-95 transition-all">
                          Approve Tone
                        </Button>
                        <Button className="px-md py-sm border border-outline-variant text-on-surface rounded-lg font-label-md hover:bg-surface-container-high/50 transition-all">
                          Adjust Constraints
                        </Button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            {/* Pending Node */}
            <div className="flex items-start gap-lg opacity-60 grayscale-[0.5]">
              <div className="mt-4 flex flex-col items-center">
                <div className="w-4 h-4 rounded-full border-2 border-outline-variant bg-surface-container-highest z-10"></div>
              </div>
              <div className="flex-1 glass-card bg-white/50 p-lg rounded-xl">
                <div className="flex items-center gap-md mb-xs">
                  <h4 className="font-headline-md text-on-surface-variant">
                    Social Media Scheduling
                  </h4>
                  <span className="px-sm py-xs bg-surface-container-high text-on-surface-variant font-label-sm rounded-lg flex items-center gap-1">
                    <span className="material-symbols-outlined text-[14px]">
                      lock
                    </span>{" "}
                    Pending
                  </span>
                </div>
                <p className="text-on-surface-variant">
                  Syncing finalized copy with platform-specific scheduling APIs
                  for cross-channel distribution.
                </p>
              </div>
            </div>
          </div>
        </div>

        {/* Input area */}
        <div className="absolute bottom-6 left-xl right-xl z-20 max-w-4xl mx-auto shadow-lg bg-white/90 backdrop-blur-md border border-outline-variant/30 rounded-2xl flex items-center p-xs group focus-within:border-primary focus-within:shadow-primary/10 transition-all duration-300">
            <Button className="p-3 text-on-surface-variant hover:text-primary transition-colors cursor-pointer hover:bg-surface-container-low rounded-xl">
              <span className="material-symbols-outlined" data-icon="attach_file">attach_file</span>
            </Button>
            <Input
              className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-lg py-md px-sm placeholder:text-on-surface-variant/60"
              placeholder="Add a sub-task or message the Agent..."
              type="text"
            />
            <div className="flex items-center gap-2 px-sm">
              <Button className="p-3 text-on-surface-variant hover:text-primary rounded-xl transition-colors cursor-pointer hover:bg-surface-container-low">
                <span className="material-symbols-outlined" data-icon="auto_awesome">auto_awesome</span>
              </Button>
              <Button className="bg-primary text-on-primary p-3 rounded-xl active:scale-95 transition-all shadow-md cursor-pointer hover:shadow-primary/30 flex items-center justify-center">
                <span className="material-symbols-outlined text-[20px]" data-icon="arrow_upward">arrow_upward</span>
              </Button>
            </div>
        </div>
      </div>

      {/* Right Sidebar */}
      <aside className="w-[300px] border-l border-outline-variant/20 bg-surface-container-low/30 p-lg shrink-0 flex flex-col gap-lg">
        <div className="glass-card bg-white/70 p-lg rounded-xl">
          <h5 className="font-label-md text-on-surface-variant mb-md">
            Connected Resources
          </h5>
          <div className="flex flex-wrap gap-sm">
            <span className="px-md py-sm bg-surface-container-highest rounded-full font-label-sm">
              Google Analytics 4
            </span>
            <span className="px-md py-sm bg-surface-container-highest rounded-full font-label-sm">
              Meta Ads API
            </span>
            <span className="px-md py-sm bg-surface-container-highest rounded-full font-label-sm">
              Notion Workspace
            </span>
          </div>
        </div>
        <div className="glass-card bg-white/70 p-lg rounded-xl flex flex-col justify-between">
          <div>
            <h5 className="font-label-md text-on-surface-variant mb-xs">
              Agent Efficiency Report
            </h5>
            <p className="text-on-surface-variant text-body-sm">
              Estimated time saved: <span className="text-primary font-bold">14.5 hours</span> this week.
            </p>
          </div>
          <div className="flex gap-xs items-end h-16 mt-md">
            <div className="w-4 bg-primary/20 h-[40%] rounded-t-sm"></div>
            <div className="w-4 bg-primary/40 h-[60%] rounded-t-sm"></div>
            <div className="w-4 bg-primary/60 h-[30%] rounded-t-sm"></div>
            <div className="w-4 bg-primary/80 h-[80%] rounded-t-sm"></div>
            <div className="w-4 bg-primary h-[100%] rounded-t-sm"></div>
          </div>
        </div>
      </aside>
    </div>
  );
}
