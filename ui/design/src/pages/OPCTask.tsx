import { Link } from 'react-router-dom';
import { cn } from '../lib/utils';
import { Button } from "@/components/ui/button";

export default function OPCTask() {
  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full px-lg py-xl">
      <div className="max-w-[1400px] mx-auto animate-in fade-in duration-700">

        <div className="grid grid-cols-1 xl:grid-cols-12 gap-lg pb-10">

          {/* Left Column (Main Content) */}
          <div className="xl:col-span-8 flex flex-col gap-lg">

             {/* Agent Workflow Card */}
             <div className="bg-white rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
                <div className="flex items-center gap-2 mb-8">
                  <span className="material-symbols-outlined text-[20px] text-on-surface">account_tree</span>
                  <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Agent Workflow</h3>
                </div>

                <div className="relative flex items-center justify-between mb-10 px-4 md:px-10">
                  {/* Connecting Line Base */}
                  <div className="absolute left-10 md:left-16 right-10 md:right-16 top-6 h-0.5 bg-outline-variant/20 z-0"></div>
                  {/* Connecting Line Active */}
                  <div className="absolute left-10 md:left-16 w-1/2 top-6 h-0.5 bg-primary z-0"></div>

                  {/* Step 1 */}
                  <div className="relative z-10 flex flex-col items-center gap-2">
                    <div className="w-12 h-12 rounded-full border border-primary bg-white text-primary flex items-center justify-center shrink-0">
                      <span className="material-symbols-outlined text-[20px]">emoji_objects</span>
                    </div>
                    <span className="font-label-sm text-[12px] text-on-surface">CTO</span>
                  </div>

                  {/* Step 2 */}
                  <div className="relative z-10 flex flex-col items-center gap-2">
                    <div className="w-12 h-12 rounded-full border border-outline-variant bg-surface-container-lowest text-on-surface flex items-center justify-center shrink-0">
                      <span className="material-symbols-outlined text-[20px]">badge</span>
                    </div>
                    <span className="font-label-sm text-[12px] text-on-surface-variant">Product Mgr</span>
                  </div>

                  {/* Step 3 (Active) */}
                  <div className="relative z-10 flex flex-col items-center gap-2">
                    <div className="w-16 h-16 rounded-full bg-primary/10 flex items-center justify-center -mt-2 shrink-0">
                      <div className="w-12 h-12 rounded-full bg-primary text-white flex items-center justify-center shadow-md">
                        <span className="material-symbols-outlined text-[20px]">code</span>
                      </div>
                    </div>
                    <span className="font-label-sm text-[12px] text-primary font-bold">SDE (Active)</span>
                  </div>

                  {/* Step 4 */}
                  <div className="relative z-10 flex flex-col items-center gap-2">
                    <div className="w-12 h-12 rounded-full border border-outline-variant bg-surface-container-lowest text-on-surface-variant flex items-center justify-center shrink-0">
                      <span className="material-symbols-outlined text-[20px]">verified_user</span>
                    </div>
                    <span className="font-label-sm text-[12px] text-on-surface-variant">QA Specialist</span>
                  </div>
                </div>

                {/* Score Bar */}
                <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/30 flex flex-col gap-2 shadow-sm">
                  <div className="flex justify-between items-center text-sm">
                    <span className="font-body-sm text-[13px] text-on-surface">Agent Harmony Score</span>
                    <span className="font-label-md text-[14px] font-bold text-primary">98%</span>
                  </div>
                  <div className="h-2 w-full bg-surface-container rounded-full overflow-hidden">
                    <div className="h-full bg-primary rounded-full w-[98%]"></div>
                  </div>
                </div>
             </div>

             {/* Task Description */}
             <div className="bg-white rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
                <div className="flex items-center gap-2 mb-6">
                  <span className="material-symbols-outlined text-[20px] text-on-surface">description</span>
                  <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Task Description</h3>
                  <span className="material-symbols-outlined text-[16px] text-on-surface-variant ml-1 cursor-pointer hover:text-primary transition-colors">edit</span>
                </div>
                <div className="font-body-md text-[15px] text-on-surface-variant space-y-4 leading-relaxed">
                   <p>The objective is to completely overhaul the landing page hero section to improve conversion rates and LCP (Largest Contentful Paint) metrics. This involves implementing a new responsive layout, optimizing asset delivery, and refining the visual hierarchy using the latest design system tokens.</p>
                   <p>Key requirements include a dynamic background element, clear call-to-action buttons with hover states, and a performance-first approach to typography and image loading.</p>
                </div>
             </div>

             {/* Execution Log */}
             <div className="bg-white rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
                <div className="flex items-center justify-between mb-8">
                  <div className="flex items-center gap-2">
                    <span className="material-symbols-outlined text-[20px] text-on-surface">receipt_long</span>
                    <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Execution Log</h3>
                  </div>
                  <span className="bg-surface-container-low text-on-surface-variant font-label-sm text-[11px] px-3 py-1 rounded-full border border-outline-variant/20">32 Events</span>
                </div>

                <div className="relative pl-0 md:pl-2 space-y-10">
                   {/* Vertical Line */}
                   <div className="absolute left-[15px] md:left-[23px] top-4 bottom-8 w-px bg-outline-variant/30"></div>

                   {/* Event 1 */}
                   <div className="relative flex items-start gap-4">
                      <div className="w-8 h-8 rounded-full border-2 border-outline-variant/40 bg-white text-primary flex items-center justify-center shrink-0 relative z-10 md:ml-2">
                         <span className="material-symbols-outlined text-[16px]">emoji_objects</span>
                      </div>
                      <div className="flex-1 -mt-1">
                         <div className="flex justify-between items-start mb-1">
                            <h4 className="font-label-md text-[14px] text-primary">CTO Initialized Proposal</h4>
                            <span className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">10:24:12 AM</span>
                         </div>
                         <p className="text-body-sm text-[14px] mt-1 text-on-surface-variant leading-relaxed">Generated technical specification document for the landing page hero revamp. Identified key performance metrics for LCP improvement.</p>
                      </div>
                   </div>

                   {/* Event 2 */}
                   <div className="relative flex items-start gap-4">
                      <div className="w-8 h-8 rounded-full border-2 border-outline-variant/40 bg-white text-on-surface-variant flex items-center justify-center shrink-0 relative z-10 md:ml-2">
                         <span className="material-symbols-outlined text-[16px]">badge</span>
                      </div>
                      <div className="flex-1 -mt-1">
                         <div className="flex justify-between items-start mb-1">
                            <h4 className="font-label-md text-[14px] text-on-surface">PM Defined User Stories</h4>
                            <span className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">10:25:45 AM</span>
                         </div>
                         <p className="text-body-sm text-[14px] mt-1 text-on-surface-variant leading-relaxed">Broke down technical spec into 4 core sprint tasks. Assigned creative assets requirement to designer node.</p>
                      </div>
                   </div>

                   {/* Event 3 (Active) */}
                   <div className="relative flex items-start gap-4">
                      <div className="w-8 h-8 rounded-full bg-primary text-white flex items-center justify-center shrink-0 relative z-10 md:ml-2 shadow-sm ring-4 ring-primary/10">
                         <span className="material-symbols-outlined text-[16px]">code</span>
                      </div>
                      <div className="flex-1 -mt-1">
                         <div className="flex justify-between items-center mb-1">
                            <h4 className="font-label-md text-[14px] text-primary font-bold">SDE Committing Code</h4>
                            <span className="font-label-sm text-[10px] text-primary font-bold uppercase tracking-wider">LIVE</span>
                         </div>
                         <p className="text-body-sm text-[14px] mt-1 text-on-surface mb-4 leading-relaxed">Applying Tailwind CSS refinements to the hero container. Pushing initial layout changes to branch <code className="bg-surface-container text-primary font-mono text-[13px] px-1.5 py-0.5 rounded">feature/hero-revamp-v2</code>.</p>
                         <div className="flex items-center gap-3">
                            <div className="bg-surface-container px-2 py-1 rounded gap-2 flex items-center font-mono text-[11px] font-bold">
                               <span className="text-on-surface">+142 -22 lines</span>
                            </div>
                            <div className="bg-primary/10 border border-primary/20 text-primary px-2 py-1 rounded flex items-center font-mono text-[11px] font-bold">
                               lint: success
                            </div>
                         </div>
                      </div>
                   </div>

                   {/* Event 4 (Pending) */}
                   <div className="relative flex items-start gap-4 opacity-50">
                      <div className="w-8 h-8 rounded-full border-2 border-dashed border-outline-variant/60 bg-surface text-on-surface-variant flex items-center justify-center shrink-0 relative z-10 md:ml-2">
                         <span className="material-symbols-outlined text-[16px]">verified_user</span>
                      </div>
                      <div className="flex-1 -mt-1">
                         <div className="flex justify-between items-start mb-1">
                            <h4 className="font-label-md text-[14px] text-on-surface-variant">QA Verification Pending</h4>
                            <span className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">Scheduled</span>
                         </div>
                         <p className="text-body-sm text-[14px] mt-1 text-on-surface-variant italic">Waiting for SDE to finalize deployment to sandbox cluster...</p>
                      </div>
                   </div>
                </div>
             </div>

          </div>

          {/* Right Column (Sidebar Panels) */}
          <div className="xl:col-span-4 flex flex-col gap-lg">

             {/* HUMAN-IN-THE-LOOP Panel */}
             <div className="border border-primary/30 bg-white rounded-2xl p-xl shadow-sm relative overflow-hidden">
                <div className="absolute top-0 left-0 right-0 h-1 bg-gradient-to-r from-primary to-secondary"></div>
                <div className="flex items-center gap-2 mb-6 mt-1">
                  <span className="material-symbols-outlined text-primary text-[20px]">lock_open</span>
                  <h3 className="font-label-md text-[14px] font-bold text-primary uppercase tracking-widest">HUMAN-IN-THE-LOOP REQUIRED</h3>
                </div>

                <Button className="w-full bg-primary text-white py-3 rounded-xl font-label-md font-bold text-[14px] mb-4 flex items-center justify-center gap-2 shadow-sm hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer">
                  <span className="material-symbols-outlined text-[20px]">call_merge</span>
                  Approve Final Merge
                </Button>

                <div className="flex gap-3 mb-6">
                  <Button className="flex-1 bg-white border border-outline-variant/30 text-on-surface py-2.5 rounded-xl font-label-md text-[13px] font-bold flex items-center justify-center gap-2 hover:bg-surface-container-low transition-colors cursor-pointer">
                    <span className="material-symbols-outlined text-[18px]">history</span>
                    Rollback
                  </Button>
                  <Button className="flex-1 bg-white border border-outline-variant/30 text-on-surface py-2.5 rounded-xl font-label-md text-[13px] font-bold flex items-center justify-center gap-2 hover:bg-surface-container-low transition-colors cursor-pointer">
                    <span className="material-symbols-outlined text-[18px]">edit_note</span>
                    Revision
                  </Button>
                </div>

                <p className="font-label-sm text-[11px] text-on-surface-variant leading-relaxed text-center opacity-80">
                  Finalizing this action will trigger automatic deployment to Vercel/Production.
                </p>
             </div>

             {/* Task Artifacts */}
             <div className="bg-white rounded-2xl p-xl border border-outline-variant/30 shadow-sm flex flex-col gap-md">
                <div className="flex items-center gap-2 mb-2">
                  <span className="material-symbols-outlined text-[20px] text-primary">inventory_2</span>
                  <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Task Artifacts</h3>
                </div>

                <div className="border border-outline-variant/30 rounded-xl p-4 flex items-start gap-4 hover:border-primary/40 hover:bg-surface-container-lowest transition-colors cursor-pointer group">
                  <div className="w-10 h-10 rounded-lg bg-surface-container flex items-center justify-center shrink-0 text-on-surface-variant group-hover:text-primary group-hover:bg-primary/10 transition-colors">
                    <span className="material-symbols-outlined text-[20px]">link</span>
                  </div>
                  <div>
                    <div className="font-label-md text-[14px] font-bold text-on-surface mb-0.5 group-hover:text-primary transition-colors">Preview Sandbox</div>
                    <div className="font-label-sm text-[11px] text-on-surface-variant">https://dev-hero-revamp.aether.ai</div>
                  </div>
                </div>

                <div className="border border-outline-variant/30 rounded-xl p-4 flex items-start gap-4 hover:border-primary/40 hover:bg-surface-container-lowest transition-colors cursor-pointer group">
                  <div className="w-10 h-10 rounded-lg bg-surface-container flex items-center justify-center shrink-0 text-on-surface-variant group-hover:text-primary group-hover:bg-primary/10 transition-colors">
                    <span className="material-symbols-outlined text-[20px]">integration_instructions</span>
                  </div>
                  <div>
                    <div className="font-label-md text-[14px] font-bold text-on-surface mb-0.5 group-hover:text-primary transition-colors">Pull Request #412</div>
                    <div className="font-label-sm text-[11px] text-on-surface-variant">Status: Draft (Pending Final Polish)</div>
                  </div>
                </div>
             </div>

             {/* Efficiency Metrics */}
             <div className="bg-white rounded-2xl p-xl border border-outline-variant/30 shadow-sm flex flex-col gap-md">
                <div className="flex items-center gap-2 mb-2">
                  <span className="material-symbols-outlined text-[20px] text-primary">monitoring</span>
                  <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Efficiency Metrics</h3>
                </div>

                <div className="grid grid-cols-2 gap-sm">
                   <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                      <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Compute Cost</div>
                      <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">$1.42</div>
                      <div className="font-label-sm text-[10px] text-green-600 font-bold flex items-center gap-1">
                        <span className="material-symbols-outlined text-[14px]">trending_down</span> -12% avg
                      </div>
                   </div>

                   <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                      <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Token Usage</div>
                      <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">14.8k</div>
                      <div className="font-label-sm text-[10px] text-on-surface-variant">
                        GPT-4o / Claude 3.5
                      </div>
                   </div>

                   <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                      <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Duration</div>
                      <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">1h 12m</div>
                      <div className="font-label-sm text-[10px] text-on-surface-variant">
                        Est. human time: 6h
                      </div>
                   </div>

                   <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                      <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Agent Load</div>
                      <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">Med-High</div>
                      <div className="font-label-sm text-[10px] text-on-surface-variant line-clamp-1">
                        Parallel processing enabled
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
