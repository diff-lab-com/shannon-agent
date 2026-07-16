import React from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";

export default function Chat() {
  return (
    <div className="flex-1 flex w-full h-full relative">
      {/* Left Sidebar - Chat History */}
      <aside className="w-[240px] border-r border-outline-variant/10 flex flex-col glass-panel shrink-0 bg-white/40">
        <div className="p-md border-b border-outline-variant/10">
          <div className="relative">
            <span
              className="material-symbols-outlined absolute left-sm top-1/2 -translate-y-1/2 text-on-surface-variant text-[18px]"
              data-icon="search"
            >
              search
            </span>
            <Input
              className="w-full pl-xl pr-md py-xs bg-surface-container border-none rounded-lg text-body-sm focus:ring-1 focus:ring-primary/30"
              placeholder="Search sessions..."
              type="text"
            />
          </div>
        </div>
        <div className="flex-1 overflow-y-auto p-sm space-y-xs">
          <div className="text-label-sm text-outline px-sm py-xs uppercase tracking-widest opacity-60">
            Today
          </div>
          <div className="p-sm rounded-lg bg-primary-fixed/40 border-l-4 border-primary shadow-sm cursor-pointer">
            <p className="font-label-md text-primary font-bold truncate">
              Market Trends 2026
            </p>
            <p className="text-body-sm text-on-surface-variant opacity-70 truncate">
              Drafting detailed forecast...
            </p>
          </div>
          <div className="p-sm rounded-lg hover:bg-surface-container-high/50 cursor-pointer group">
            <p className="font-label-md text-on-surface truncate group-hover:text-primary transition-colors">
              Neural Network Architecture
            </p>
            <p className="text-body-sm text-on-surface-variant opacity-70 truncate">
              Session ended yesterday
            </p>
          </div>
          <div className="p-sm rounded-lg hover:bg-surface-container-high/50 cursor-pointer group">
            <p className="font-label-md text-on-surface truncate group-hover:text-primary transition-colors">
              Project: Aether UI Kit
            </p>
            <p className="text-body-sm text-on-surface-variant opacity-70 truncate">
              Exported components
            </p>
          </div>
        </div>
      </aside>

      {/* Main Chat Canvas */}
      <section className="flex-1 flex flex-col relative bg-white/40 overflow-hidden">
        {/* Actionable Breadcrumbs */}
        <div className="px-xl py-md flex items-center gap-sm border-b border-outline-variant/10 bg-white/20 backdrop-blur-sm sticky top-0 z-10">
          <div className="flex items-center gap-xs px-md py-sm bg-surface-container-low rounded-full cursor-pointer hover:bg-surface-container-high/50">
            <span
              className="material-symbols-outlined text-[16px] text-primary"
              data-icon="search"
            >
              search
            </span>
            <span className="text-label-sm">Search</span>
          </div>
          <span
            className="material-symbols-outlined text-outline-variant text-[16px]"
            data-icon="chevron_right"
          >
            chevron_right
          </span>
          <div className="flex items-center gap-xs px-md py-sm bg-surface-container-low rounded-full cursor-pointer hover:bg-surface-container-high/50">
            <span
              className="material-symbols-outlined text-[16px] text-primary"
              data-icon="filter_list"
            >
              filter_list
            </span>
            <span className="text-label-sm">Filter</span>
          </div>
          <span
            className="material-symbols-outlined text-outline-variant text-[16px]"
            data-icon="chevron_right"
          >
            chevron_right
          </span>
          <div className="flex items-center gap-xs px-md py-sm bg-primary-container text-on-primary-container rounded-full shadow-sm cursor-pointer hover:opacity-90">
            <span
              className="material-symbols-outlined text-[16px]"
              data-icon="summarize"
            >
              summarize
            </span>
            <span className="text-label-sm font-bold">Summarize</span>
          </div>
        </div>

        {/* Conversation Scroll Area */}
        <ScrollArea className="flex-1 p-xl space-y-xl pb-32">
          {/* User Message */}
          <div className="flex justify-end">
            <div className="max-w-[80%] bg-primary-fixed text-on-primary-fixed px-lg py-md rounded-2xl rounded-tr-none shadow-sm">
              <p className="font-body-md">
                Can you analyze the projected market trends for 2026, specifically
                focusing on renewable energy and decentralized finance
                integration?
              </p>
            </div>
          </div>

          {/* Agent Message */}
          <div className="flex gap-md max-w-[90%]">
            <div className="h-10 w-10 rounded-full bg-primary-container flex items-center justify-center shrink-0 shadow-md">
              <span
                className="material-symbols-outlined text-on-primary-container"
                data-icon="smart_toy"
              >
                smart_toy
              </span>
            </div>
            <div className="space-y-lg flex-1">
              <div className="bg-white px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm space-y-md">
                <p className="font-body-md text-on-surface">
                  I've completed a multi-dimensional analysis. By 2026, we
                  anticipate a significant convergence where DeFi protocols will
                  facilitate fractionalized ownership of micro-generation grids.
                </p>

                {/* AI Thought Steps */}
                <div className="bg-surface-container-lowest p-md rounded-xl border border-outline-variant/10 space-y-md">
                  <div className="relative pl-6">
                    <div className="thought-connector absolute left-[7px] top-[14px]"></div>
                    <div className="absolute left-0 top-1 h-4 w-4 rounded-full bg-primary/20 flex items-center justify-center">
                      <div className="h-1.5 w-1.5 rounded-full bg-primary"></div>
                    </div>
                    <span className="font-label-sm text-label-sm text-on-surface-variant block uppercase opacity-70">
                      Knowledge Retrieval
                    </span>
                    <p className="text-body-sm">
                      Aggregating Q4 2024 reports and 2025 strategic forecasts.
                    </p>
                  </div>
                  <div className="relative pl-6">
                    <div className="thought-connector absolute left-[7px] top-[14px]"></div>
                    <div className="absolute left-0 top-1 h-4 w-4 rounded-full bg-primary/20 flex items-center justify-center">
                      <div className="h-1.5 w-1.5 rounded-full bg-primary"></div>
                    </div>
                    <span className="font-label-sm text-label-sm text-on-surface-variant block uppercase opacity-70">
                      Pattern Synthesis
                    </span>
                    <p className="text-body-sm">
                      Identifying correlation between solar storage costs and
                      blockchain throughput.
                    </p>
                  </div>
                  <div className="relative pl-6">
                    <div className="absolute left-0 top-1 h-4 w-4 rounded-full bg-primary/20 flex items-center justify-center">
                      <div className="h-1.5 w-1.5 rounded-full bg-primary"></div>
                    </div>
                    <span className="font-label-sm text-label-sm text-on-surface-variant block uppercase opacity-70">
                      Drafting Response
                    </span>
                    <p className="text-body-sm">
                      Compiling summary of actionable investment vectors.
                    </p>
                  </div>
                </div>

                <p className="font-body-md text-on-surface">
                  Key findings suggest a 42% increase in tokenized energy assets.
                  Would you like me to breakdown the regulatory hurdles expected
                  in the EU sector?
                </p>
              </div>

              <div className="flex gap-sm">
                <Button className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
                  <span
                    className="material-symbols-outlined text-[18px]"
                    data-icon="thumb_up"
                  >
                    thumb_up
                  </span>
                </Button>
                <Button className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
                  <span
                    className="material-symbols-outlined text-[18px]"
                    data-icon="content_copy"
                  >
                    content_copy
                  </span>
                </Button>
                <Button className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
                  <span
                    className="material-symbols-outlined text-[18px]"
                    data-icon="refresh"
                  >
                    refresh
                  </span>
                </Button>
              </div>
            </div>
          </div>
        </ScrollArea>

        {/* Command Bar Area */}
        <div className="absolute bottom-6 md:bottom-12 w-full px-lg md:px-xl py-lg bg-gradient-to-t from-background via-background/90 to-transparent">
          <div className="max-w-4xl mx-auto relative group">
            <div className="absolute inset-0 bg-primary/10 blur-xl rounded-full opacity-50 group-focus-within:opacity-100 transition-opacity duration-500"></div>
            <div className="relative glass-card bg-white/80 rounded-2xl border border-outline-variant/30 px-sm py-xs flex items-center shadow-lg group-focus-within:border-primary/50 group-focus-within:shadow-primary/10 transition-all duration-300">
              <span
                className="material-symbols-outlined p-md text-primary animate-pulse"
                data-icon="auto_awesome"
              >
                auto_awesome
              </span>
              <Input
                className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-lg py-md px-sm placeholder:text-outline-variant/80 text-on-surface"
                placeholder="Ask Aether about the EU regulatory impact..."
                type="text"
              />
              <div className="flex items-center gap-2 px-sm">
                <Button className="p-3 text-on-surface-variant hover:text-primary hover:bg-surface-container-low rounded-xl transition-colors cursor-pointer">
                  <span className="material-symbols-outlined" data-icon="attach_file">
                    attach_file
                  </span>
                </Button>
                <Button className="bg-primary text-on-primary p-3 rounded-xl active:scale-95 hover:shadow-md hover:shadow-primary/30 transition-all cursor-pointer flex items-center justify-center">
                  <span className="material-symbols-outlined text-[20px]" data-icon="arrow_upward">
                    arrow_upward
                  </span>
                </Button>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Environmental Awareness (Right Sidebar) */}
      <aside className="w-[300px] border-l border-outline-variant/10 glass-panel shrink-0 p-lg overflow-y-auto bg-white/50">
        <div className="space-y-xl">
          {/* Context Section */}
          <section>
            <div className="flex items-center justify-between mb-md">
              <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60">
                File Context
              </h3>
              <span className="px-xs py-[2px] bg-tertiary-fixed text-on-tertiary-fixed text-[10px] font-bold rounded">
                4 ACTIVE
              </span>
            </div>
            <div className="space-y-sm">
              <div className="p-md bg-surface-container rounded-xl flex items-center gap-md border border-outline-variant/10">
                <span
                  className="material-symbols-outlined text-tertiary"
                  data-icon="description"
                >
                  description
                </span>
                <div className="min-w-0">
                  <p className="text-label-md truncate">Global_Trends_2025.pdf</p>
                  <p className="text-label-sm opacity-60">
                    PDF Document • 1.2 MB
                  </p>
                </div>
              </div>
              <div className="p-md bg-surface-container rounded-xl flex items-center gap-md border border-outline-variant/10">
                <span
                  className="material-symbols-outlined text-primary"
                  data-icon="table_chart"
                >
                  table_chart
                </span>
                <div className="min-w-0">
                  <p className="text-label-md truncate">
                    Renewable_Forecast_Raw.csv
                  </p>
                  <p className="text-label-sm opacity-60">
                    Spreadsheet • 4.5k rows
                  </p>
                </div>
              </div>
            </div>
          </section>

          {/* Active Skills */}
          <section>
            <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">
              Active Skills
            </h3>
            <div className="flex flex-wrap gap-xs">
              <span className="px-md py-sm bg-primary/10 text-primary text-label-sm rounded-full border border-primary/20 flex items-center gap-xs">
                <span
                  className="material-symbols-outlined text-[14px]"
                  data-icon="query_stats"
                >
                  query_stats
                </span>
                Forecasting
              </span>
              <span className="px-md py-sm bg-primary/10 text-primary text-label-sm rounded-full border border-primary/20 flex items-center gap-xs">
                <span
                  className="material-symbols-outlined text-[14px]"
                  data-icon="translate"
                >
                  translate
                </span>
                Cross-Lingual
              </span>
              <span className="px-md py-sm bg-primary/10 text-primary text-label-sm rounded-full border border-primary/20 flex items-center gap-xs">
                <span
                  className="material-symbols-outlined text-[14px]"
                  data-icon="account_balance"
                >
                  account_balance
                </span>
                Regulatory Compliance
              </span>
            </div>
          </section>

          {/* Agent Visual Insight */}
          <section className="space-y-md">
            <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60">
              Visual Analysis
            </h3>
            <div className="aspect-square rounded-2xl overflow-hidden shadow-inner border border-outline-variant/20 relative group bg-black">
              <img
                className="w-full h-full object-cover opacity-80"
                alt="Projected Energy Asset Growth chart"
                src="https://images.unsplash.com/photo-1551288049-bebda4e38f71?auto=format&fit=crop&q=80&w=400&h=400"
              />
              <div className="absolute inset-0 bg-primary/20 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center backdrop-blur-[2px]">
                <Button className="bg-white text-primary px-lg py-md rounded-xl font-label-md shadow-lg cursor-pointer">
                  Expand Visual
                </Button>
              </div>
            </div>
            <p className="text-body-sm text-center opacity-60">
              Projected Energy Asset Growth (2024-2026)
            </p>
          </section>
        </div>
      </aside>
    </div>
  );
}
