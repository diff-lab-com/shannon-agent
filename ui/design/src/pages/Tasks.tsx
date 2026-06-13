import React, { useState } from 'react';
import { cn } from '../lib/utils';
import { Button } from "@/components/ui/button";

export default function Tasks() {
  const [running, setRunning] = useState(false);

  const handleRunNow = () => {
    // Mock run interaction
    setRunning(true);
    setTimeout(() => setRunning(false), 2000);
  };

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        {/* Page Header */}
        <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface">Scheduled Tasks</h2>
            <p className="text-on-surface-variant mt-xs">Manage and monitor your automated intelligence workflows.</p>
          </div>
          <div className="flex gap-sm">
            <Button className="px-md py-sm rounded-xl border border-outline-variant/50 flex items-center gap-sm hover:bg-surface-container-high/30 transition-all font-label-md cursor-pointer">
              <span className="material-symbols-outlined text-[20px]">filter_list</span>
              Filters
            </Button>
            <Button className="px-md py-sm rounded-xl border border-outline-variant/50 flex items-center gap-sm hover:bg-surface-container-high/30 transition-all font-label-md cursor-pointer">
              <span className="material-symbols-outlined text-[20px]">calendar_today</span>
              Month View
            </Button>
          </div>
        </div>

        {/* Bento Grid Layout */}
        <div className="grid grid-cols-12 gap-gutter">
          {/* Tasks List (Col 1-8) */}
          <div className="col-span-12 lg:col-span-8 space-y-md">
            {/* Task Item 1 */}
            <div className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-300 group bg-white/80">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-md">
                  <div className="w-12 h-12 rounded-xl bg-primary/10 flex items-center justify-center text-primary">
                    <span className="material-symbols-outlined text-[28px]">newspaper</span>
                  </div>
                  <div>
                    <h3 className="font-body-lg font-semibold text-on-surface group-hover:text-primary transition-colors">Daily News Briefing</h3>
                    <div className="flex items-center gap-md mt-1">
                      <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]">schedule</span>
                        8:00 AM Daily
                      </span>
                      <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]">smart_toy</span>
                        ResearchBot
                      </span>
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-lg">
                  <div className="flex items-center gap-xs px-sm py-1 rounded-full bg-emerald-100 text-emerald-800 border border-emerald-200">
                    <span className="w-2 h-2 rounded-full bg-emerald-500"></span>
                    <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">Scheduled</span>
                  </div>
                  <div className="flex items-center gap-sm">
                    <Button className="p-2 rounded-lg hover:bg-surface-container-high text-on-surface-variant transition-colors cursor-pointer" title="Edit">
                      <span className="material-symbols-outlined">edit</span>
                    </Button>
                    <Button
                      onClick={handleRunNow}
                      className={cn(
                        "text-on-primary px-md py-sm rounded-lg font-label-md flex items-center gap-xs hover:brightness-110 active:scale-95 transition-all cursor-pointer",
                        running ? "bg-emerald-500" : "bg-primary"
                      )}
                    >
                      {running ? (
                        <>
                          <span className="material-symbols-outlined text-[18px]">check_circle</span>
                          Success
                        </>
                      ) : (
                        <>
                          <span className="material-symbols-outlined text-[18px]">play_arrow</span>
                          Run Now
                        </>
                      )}
                    </Button>
                  </div>
                </div>
              </div>
            </div>

            {/* Task Item 2 */}
            <div className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-300 group bg-white/80">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-md">
                  <div className="w-12 h-12 rounded-xl bg-secondary/10 flex items-center justify-center text-secondary">
                    <span className="material-symbols-outlined text-[28px]">payments</span>
                  </div>
                  <div>
                    <h3 className="font-body-lg font-semibold text-on-surface group-hover:text-secondary transition-colors">Weekly Expense Report</h3>
                    <div className="flex items-center gap-md mt-1">
                      <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]">event</span>
                        Every Friday, 5:00 PM
                      </span>
                      <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]">smart_toy</span>
                        FinanceAgent
                      </span>
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-lg">
                  <div className="flex items-center gap-xs px-sm py-1 rounded-full bg-blue-100 text-blue-800 border border-blue-200">
                    <span className="w-2 h-2 rounded-full bg-blue-500 animate-pulse"></span>
                    <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">Running</span>
                  </div>
                  <div className="flex items-center gap-sm">
                    <Button className="p-2 rounded-lg hover:bg-surface-container-high text-on-surface-variant transition-colors cursor-pointer">
                      <span className="material-symbols-outlined">stop_circle</span>
                    </Button>
                    <Button className="bg-surface-variant text-on-surface-variant px-md py-sm rounded-lg font-label-md flex items-center gap-xs opacity-50 cursor-not-allowed">
                      <span className="material-symbols-outlined text-[18px]">play_arrow</span>
                      Run Now
                    </Button>
                  </div>
                </div>
              </div>
            </div>

            {/* Task Item 3 */}
            <div className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-300 group bg-white/80">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-md">
                  <div className="w-12 h-12 rounded-xl bg-tertiary/10 flex items-center justify-center text-tertiary">
                    <span className="material-symbols-outlined text-[28px]">search_check</span>
                  </div>
                  <div>
                    <h3 className="font-body-lg font-semibold text-on-surface group-hover:text-tertiary transition-colors">LinkedIn Profile Monitoring</h3>
                    <div className="flex items-center gap-md mt-1">
                      <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]">update</span>
                        Every 4 hours
                      </span>
                      <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]">smart_toy</span>
                        NetworkBot
                      </span>
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-lg">
                  <div className="flex items-center gap-xs px-sm py-1 rounded-full bg-surface-container-highest text-on-surface-variant border border-outline-variant/30">
                    <span className="w-2 h-2 rounded-full bg-outline"></span>
                    <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">Paused</span>
                  </div>
                  <div className="flex items-center gap-sm">
                    <Button className="p-2 rounded-lg hover:bg-surface-container-high text-on-surface-variant transition-colors cursor-pointer">
                      <span className="material-symbols-outlined">play_circle</span>
                    </Button>
                    <Button className="bg-primary text-on-primary px-md py-sm rounded-lg font-label-md flex items-center gap-xs hover:brightness-110 active:scale-95 transition-all cursor-pointer">
                      <span className="material-symbols-outlined text-[18px]">play_arrow</span>
                      Run Now
                    </Button>
                  </div>
                </div>
              </div>
            </div>

            {/* Execution Log */}
            <div className="pt-lg">
              <h4 className="font-label-md text-label-md text-outline uppercase tracking-[0.1em] mb-md pl-xs">Task Execution Log</h4>
              <div className="relative pl-8 border-l border-outline-variant/30 space-y-lg ml-md">
                <div className="relative">
                  <div className="absolute -left-[41px] top-1 w-4 h-4 rounded-full border-2 border-primary bg-white z-10"></div>
                  <p className="font-label-sm text-label-sm text-primary mb-1">08:00:02 — COMPLETED</p>
                  <p className="text-on-surface-variant text-body-sm italic">Daily News Briefing successfully aggregated from 14 sources.</p>
                </div>
                <div className="relative">
                  <div className="absolute -left-[41px] top-1 w-4 h-4 rounded-full border-2 border-blue-500 bg-white z-10 animate-pulse"></div>
                  <p className="font-label-sm text-label-sm text-blue-500 mb-1">10:15:45 — PROCESSING</p>
                  <p className="text-on-surface-variant text-body-sm">Weekly Expense Report is currently reconciling receipt OCR data...</p>
                </div>
              </div>
            </div>
          </div>

          {/* Calendar Preview & Secondary Insights (Col 9-12) */}
          <div className="col-span-12 lg:col-span-4 space-y-gutter">
            {/* Calendar Widget */}
            <div className="bg-white border border-outline-variant/30 rounded-2xl p-lg shadow-sm">
              <div className="flex items-center justify-between mb-lg">
                <h4 className="font-headline-md text-[18px] text-on-surface">Schedule</h4>
                <div className="flex gap-sm">
                  <span className="material-symbols-outlined text-on-surface-variant text-[20px] cursor-pointer">chevron_left</span>
                  <span className="material-symbols-outlined text-on-surface-variant text-[20px] cursor-pointer">chevron_right</span>
                </div>
              </div>

              <div className="grid grid-cols-7 text-center mb-sm">
                <span className="text-[10px] font-bold text-outline uppercase">Mo</span>
                <span className="text-[10px] font-bold text-outline uppercase">Tu</span>
                <span className="text-[10px] font-bold text-outline uppercase">We</span>
                <span className="text-[10px] font-bold text-outline uppercase">Th</span>
                <span className="text-[10px] font-bold text-outline uppercase">Fr</span>
                <span className="text-[10px] font-bold text-outline uppercase">Sa</span>
                <span className="text-[10px] font-bold text-outline uppercase">Su</span>
              </div>

              <div className="grid grid-cols-7 gap-1 text-center font-label-md">
                <span className="py-2 text-outline/30">28</span>
                <span className="py-2 text-outline/30">29</span>
                <span className="py-2 text-outline/30">30</span>
                <span className="py-2 text-outline/30">31</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">1</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">2</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">3</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">4</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">5</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">6</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">7</span>
                <div className="py-2 bg-primary text-on-primary rounded-lg cursor-pointer font-bold relative">
                  8
                  <div className="absolute bottom-1 left-1/2 -translate-x-1/2 w-1 h-1 bg-white rounded-full"></div>
                </div>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">9</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">10</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">11</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">12</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">13</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">14</span>
                <span className="py-2 bg-primary-container/20 text-primary font-bold rounded-lg cursor-pointer relative">
                  15
                  <div className="absolute bottom-1 left-1/2 -translate-x-1/2 w-1 h-1 bg-primary rounded-full"></div>
                </span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">16</span>
                <span className="py-2 hover:bg-surface-container rounded-lg cursor-pointer">17</span>
              </div>

              <div className="mt-lg pt-lg border-t border-outline-variant/20">
                <h5 className="font-label-sm text-outline uppercase tracking-wider mb-md">Upcoming Today</h5>
                <div className="space-y-md">
                  <div className="flex items-start gap-md">
                    <div className="w-1 bg-secondary h-8 rounded-full"></div>
                    <div>
                      <p className="text-body-sm font-semibold">Weekly Expense Report</p>
                      <p className="text-[12px] text-on-surface-variant">05:00 PM</p>
                    </div>
                  </div>
                  <div className="flex items-start gap-md">
                    <div className="w-1 bg-tertiary h-8 rounded-full"></div>
                    <div>
                      <p className="text-body-sm font-semibold">LinkedIn Profile Monitoring</p>
                      <p className="text-[12px] text-on-surface-variant">08:00 PM (Resume Required)</p>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            {/* Efficiency Card */}
            <div className="bg-primary overflow-hidden rounded-2xl relative p-lg text-on-primary">
              <div className="relative z-10">
                <h4 className="font-label-md text-on-primary/80 uppercase tracking-widest mb-md">AI Efficiency</h4>
                <div className="text-display-lg text-[40px] mb-xs">84%</div>
                <p className="font-body-sm text-on-primary/70">Autonomous tasks completed without human intervention this week.</p>
                <div className="mt-lg h-2 bg-white/20 rounded-full overflow-hidden">
                  <div className="h-full bg-white w-[84%]"></div>
                </div>
              </div>
              {/* Abstract Visual */}
              <div className="absolute -right-8 -bottom-8 opacity-20 transform rotate-12 pointer-events-none">
                <span className="material-symbols-outlined text-[120px]" style={{fontVariationSettings: "'FILL' 1"}}>auto_awesome</span>
              </div>
            </div>

            {/* Usage Stats Card */}
            <div className="bg-surface-container-low rounded-2xl p-lg border border-outline-variant/20">
              <h4 className="font-headline-md text-[16px] text-on-surface mb-md">Agent Allocation</h4>
              <div className="space-y-sm">
                <div>
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-body-sm text-on-surface-variant">ResearchBot</span>
                    <span className="font-label-md text-primary">42%</span>
                  </div>
                  <div className="w-full h-1 bg-outline-variant/30 rounded-full">
                    <div className="h-full bg-primary w-[42%]"></div>
                  </div>
                </div>

                <div className="pt-sm">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-body-sm text-on-surface-variant">FinanceAgent</span>
                    <span className="font-label-md text-secondary">28%</span>
                  </div>
                  <div className="w-full h-1 bg-outline-variant/30 rounded-full">
                    <div className="h-full bg-secondary w-[28%]"></div>
                  </div>
                </div>

                <div className="pt-sm">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-body-sm text-on-surface-variant">NetworkBot</span>
                    <span className="font-label-md text-tertiary">30%</span>
                  </div>
                  <div className="w-full h-1 bg-outline-variant/30 rounded-full">
                    <div className="h-full bg-tertiary w-[30%]"></div>
                  </div>
                </div>
              </div>
            </div>

          </div>
        </div>
      </div>
    </div>
  );
}
