import { Link } from 'react-router-dom';
import { cn } from '../lib/utils';
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export default function OPC() {
  const agents = [
    {
      role: 'CEO',
      function: 'Strategic Planning',
      status: 'Finalizing Q4 Roadmap',
      statusColor: 'text-primary',
      dotColor: 'bg-green-500',
      iconBg: 'bg-primary/10',
    },
    {
      role: 'CTO',
      function: 'Architecture',
      status: 'Deploying Edge V2.4',
      statusColor: 'text-secondary',
      dotColor: 'bg-green-500',
      iconBg: 'bg-secondary/10',
    },
    {
      role: 'PM',
      function: 'User Experience',
      status: 'Idle - Waiting for CEO output',
      statusColor: 'text-on-surface-variant',
      dotColor: 'bg-outline-variant',
      iconBg: 'bg-surface-container',
      icon: 'edit_square'
    },
    {
      role: 'SDE',
      function: 'Feature Implementation',
      status: 'Reviewing Code: PR-422',
      statusColor: 'text-green-600',
      dotColor: 'bg-green-500',
      iconBg: 'bg-orange-100',
    },
    {
      role: 'UI Designer',
      function: 'Experience Design',
      status: 'Designing User Flow v2.0',
      statusColor: 'text-primary',
      dotColor: 'bg-green-500',
      iconBg: 'bg-primary/10',
      icon: 'palette'
    },
    {
      role: 'QA Engineer',
      function: 'Quality Assurance',
      status: 'Running regression tests',
      statusColor: 'text-green-600',
      dotColor: 'bg-green-500',
      iconBg: 'bg-blue-100',
      icon: 'bug_report'
    },
    {
      role: 'Operations',
      function: 'Growth & Ops',
      status: 'Preparing Launch Campaign',
      statusColor: 'text-green-600',
      dotColor: 'bg-green-500',
      iconBg: 'bg-orange-50',
      icon: 'trending_up'
    },
    {
      role: 'DevOps',
      function: 'Infrastructure',
      status: 'Scaling edge clusters',
      statusColor: 'text-green-600',
      dotColor: 'bg-green-500',
      iconBg: 'bg-surface-container',
      icon: 'cloud'
    }
  ];

  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full px-lg py-xl">
      <div className="max-w-[1600px] mx-auto animate-in fade-in duration-700">

        {/* Mission Statement */}
        <div className="bg-white/70 backdrop-blur-md rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-sm">
          <div className="absolute top-lg right-lg text-primary font-label-md text-[13px] hover:underline cursor-pointer">Edit Strategic Focus</div>
          <div className="flex items-center gap-2 mb-2 uppercase font-label-md text-[13px] tracking-widest text-on-surface-variant font-bold">
             <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block"></span>
             Company Mission
          </div>
          <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-2 max-w-5xl">To build the world's most accessible decentralized AI edge computing network through autonomous agent orchestration.</h2>
        </div>

        <div className="flex flex-col lg:flex-row gap-lg items-start">

          {/* Agent Swarm List */}
          <div className="w-full lg:w-[320px] shrink-0 space-y-4">
            <div className="flex items-center gap-3">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant">Agent Swarm</h3>
              <span className="bg-secondary text-white text-[11px] font-bold px-2 py-0.5 rounded-full">12 Active</span>
            </div>

            <div className="space-y-sm">
              {agents.map((agent, i) => (
                <div key={i} className="bg-white/70 backdrop-blur-md border border-outline-variant/20 rounded-xl p-md flex flex-col shadow-sm cursor-pointer hover:border-primary/30 transition-colors group">
                  <div className="flex items-center justify-between mb-sm">
                    <div className="flex items-center gap-3">
                      <div className={cn("w-10 h-10 rounded-lg flex items-center justify-center", agent.iconBg)}>
                        {agent.icon && <span className="material-symbols-outlined text-[20px] text-on-surface-variant opacity-70">{agent.icon}</span>}
                      </div>
                      <div>
                        <div className="font-label-md text-[14px] font-bold">{agent.role}</div>
                        <div className="font-label-sm text-[11px] text-on-surface-variant">{agent.function}</div>
                      </div>
                    </div>
                    <span className={cn("w-2 h-2 rounded-full shrink-0", agent.dotColor)}></span>
                  </div>
                  <div className="flex items-center gap-2">
                    <div className={cn("w-1 h-3 rounded-full shrink-0", agent.statusColor === 'text-on-surface-variant' ? 'bg-outline-variant' : agent.statusColor.replace('text-', 'bg-'))}></div>
                    <span className={cn("font-label-sm text-[12px]", agent.statusColor, agent.statusColor === 'text-on-surface-variant' && 'italic opacity-80')}>{agent.status}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* Kanban Board */}
          <div className="flex-1 w-full flex flex-col min-w-0">
            <div className="flex justify-between items-center mb-4">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">KANBAN</h3>

              <div className="flex items-center gap-xs">
                <div className="relative">
                  <Input
                    type="text"
                    placeholder="Quick inject task..."
                    className="bg-surface-container-low border-none rounded-lg py-1.5 pl-3 pr-8 w-[200px] text-[13px] font-body-md focus:ring-2 focus:ring-primary/20 transition-all outline-none"
                  />
                  <Button className="absolute right-1 top-1/2 -translate-y-1/2 w-6 h-6 bg-primary text-white rounded-[4px] flex items-center justify-center hover:bg-primary/90 transition-colors">
                    <span className="material-symbols-outlined text-[16px]">add</span>
                  </Button>
                </div>
                <Button className="w-8 h-8 flex items-center justify-center text-on-surface-variant hover:bg-surface-container-low rounded-lg transition-colors ml-1">
                  <span className="material-symbols-outlined text-[20px]">filter_list</span>
                </Button>
              </div>
            </div>

            <div className="flex gap-4 overflow-x-auto pb-4 custom-scrollbar items-start min-h-[600px]">

              {/* To Do Column */}
              <div className="w-[300px] shrink-0 bg-surface-container-lowest/50 rounded-xl p-xs border border-transparent hover:bg-surface-container-low/30 transition-colors">
                <div className="flex justify-between items-center px-2 py-3 mb-1">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-secondary"></span>
                    <span className="font-label-md text-[14px] font-bold">To Do</span>
                  </div>
                  <span className="font-label-sm text-[11px] text-on-surface-variant">2</span>
                </div>

                <Link to="/opc/task" className="block bg-white rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-pointer hover:border-primary/50 hover:shadow-md transition-all active:scale-[0.99] group/card">
                  <div className="flex justify-between items-start mb-2">
                    <div className="w-8 h-8 rounded-lg bg-secondary/10 text-secondary flex items-center justify-center group-hover/card:bg-secondary/20 transition-colors">
                      <span className="material-symbols-outlined text-[16px]">edit_square</span>
                    </div>
                    <span className="bg-secondary/10 text-secondary text-[10px] font-bold px-2 py-0.5 rounded-[4px] uppercase tracking-wider group-hover/card:bg-secondary/20 transition-colors">Moderate</span>
                  </div>
                  <h4 className="font-label-md text-[15px] font-bold mb-3 leading-tight group-hover/card:text-primary transition-colors">Revamp Landing Page Hero</h4>
                  <div className="flex justify-between items-center mt-4">
                    <span className="font-label-sm text-[11px] text-on-surface-variant">Proposed by <strong className="text-on-surface">PM</strong></span>
                    <div className="flex items-center gap-1 text-on-surface-variant">
                      <span className="material-symbols-outlined text-[12px]">schedule</span>
                      <span className="font-label-sm text-[10px]">45m ago</span>
                    </div>
                  </div>
                </Link>

                <Button className="w-full py-2.5 border border-dashed border-outline-variant/50 rounded-xl text-on-surface-variant hover:bg-on-surface-variant/5 hover:text-on-surface transition-colors flex items-center justify-center gap-2 font-label-md text-[13px]">
                  <span className="material-symbols-outlined text-[16px]">add</span>
                  Add Task
                </Button>
              </div>

              {/* Pending Column */}
              <div className="w-[300px] shrink-0 bg-surface-container-lowest/50 rounded-xl p-xs border border-transparent hover:bg-surface-container-low/30 transition-colors">
                <div className="flex justify-between items-center px-2 py-3 mb-1">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-orange-500"></span>
                    <span className="font-label-md text-[14px] font-bold">Pending</span>
                  </div>
                  <span className="font-label-sm text-[11px] text-on-surface-variant">1</span>
                </div>

                <div className="bg-white rounded-xl p-md border border-error/20 shadow-sm mb-3 ring-1 ring-error/5 cursor-grab active:cursor-grabbing hover:border-error/40 transition-colors relative">
                  <div className="absolute left-0 top-0 bottom-0 w-1 bg-error rounded-l-xl"></div>
                  <div className="flex justify-between items-start mb-2 ml-1">
                    <div className="w-8 h-8 rounded-lg bg-primary/10 text-primary flex items-center justify-center">
                      <span className="material-symbols-outlined text-[16px]">api</span>
                    </div>
                    <span className="bg-error/10 text-error text-[10px] font-bold px-2 py-0.5 rounded-[4px] uppercase tracking-wider">Critical</span>
                  </div>
                  <h4 className="font-label-md text-[15px] font-bold mb-2 leading-tight ml-1">Upgrade API Rate Limits</h4>
                  <p className="font-body-sm text-[12px] text-on-surface-variant mb-4 ml-1 leading-snug">System load reached 88% on relay. Tier-2 upgrade proposed.</p>

                  <div className="flex justify-between items-center ml-1">
                    <div className="flex -space-x-2">
                      <div className="w-6 h-6 rounded-full bg-primary text-white flex items-center justify-center text-[9px] font-bold border border-white z-10">PM</div>
                      <div className="w-6 h-6 rounded-full bg-secondary text-white flex items-center justify-center text-[9px] font-bold border border-white z-0">CTO</div>
                    </div>
                    <span className="font-label-sm text-[12px] text-primary font-bold">Review</span>
                  </div>
                </div>
              </div>

              {/* Doing Column */}
              <div className="w-[300px] shrink-0 bg-surface-container-lowest/50 rounded-xl p-xs border border-transparent hover:bg-surface-container-low/30 transition-colors">
                <div className="flex justify-between items-center px-2 py-3 mb-1">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-primary"></span>
                    <span className="font-label-md text-[14px] font-bold">Doing</span>
                  </div>
                  <span className="font-label-sm text-[11px] text-on-surface-variant">2</span>
                </div>

                <div className="bg-white rounded-xl p-md border border-primary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:border-primary/50 transition-colors relative">
                   <div className="absolute left-0 top-0 bottom-0 w-1 bg-primary rounded-l-xl"></div>
                   <div className="flex justify-between items-center mb-2 ml-1">
                     <span className="font-label-sm text-[10px] font-bold text-primary tracking-wider">DEV-102</span>
                     <span className="material-symbols-outlined text-[16px] text-primary">autorenew</span>
                   </div>
                   <h4 className="font-label-md text-[15px] font-bold mb-4 leading-tight ml-1">Refactoring Neural Bridge v2</h4>

                   <div className="ml-1 mb-2">
                     <div className="h-1.5 w-full bg-surface-container rounded-full overflow-hidden mb-1">
                        <div className="h-full bg-primary rounded-full w-[65%]"></div>
                     </div>
                     <div className="flex justify-between items-center">
                        <span className="font-label-sm text-[10px] text-on-surface-variant">SDE Agent</span>
                        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant">65%</span>
                     </div>
                   </div>
                </div>

                <div className="bg-white rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:border-primary/50 transition-colors relative">
                   <div className="absolute left-0 top-0 bottom-0 w-1 bg-secondary rounded-l-xl"></div>
                   <div className="flex justify-between items-center mb-2 ml-1">
                     <span className="font-label-sm text-[10px] font-bold text-secondary tracking-wider">OPS-04</span>
                     <span className="material-symbols-outlined text-[16px] text-secondary">database</span>
                   </div>
                   <h4 className="font-label-md text-[15px] font-bold mb-4 leading-tight ml-1">Database Indexing Sweep</h4>

                   <div className="ml-1 mb-2">
                     <div className="h-1.5 w-full bg-surface-container rounded-full overflow-hidden mb-1">
                        <div className="h-full bg-secondary rounded-full w-[20%]"></div>
                     </div>
                     <div className="flex justify-between items-center">
                        <span className="font-label-sm text-[10px] text-on-surface-variant">CTO Agent</span>
                        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant">20%</span>
                     </div>
                   </div>
                </div>
              </div>

              {/* Done Column */}
              <div className="w-[300px] shrink-0 bg-surface-container-lowest/50 rounded-xl p-xs border border-transparent hover:bg-surface-container-low/30 transition-colors">
                <div className="flex justify-between items-center px-2 py-3 mb-1">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-green-500"></span>
                    <span className="font-label-md text-[14px] font-bold">Done</span>
                  </div>
                  <span className="font-label-sm text-[11px] text-on-surface-variant">1</span>
                </div>

                <div className="bg-white rounded-xl p-3 border border-green-500/20 shadow-sm mb-3 flex items-center justify-between cursor-pointer hover:bg-surface-bright transition-colors bg-green-50/30">
                  <div className="flex items-center gap-2">
                     <span className="material-symbols-outlined text-[16px] text-green-500">check_circle</span>
                     <span className="font-label-md text-[13px] text-on-surface">Domain Registration</span>
                  </div>
                  <span className="font-label-sm text-[10px] text-on-surface-variant">1h ago</span>
                </div>
              </div>

              {/* Deprecated Column */}
              <div className="w-[300px] shrink-0 bg-surface-container-lowest/50 rounded-xl p-xs border border-transparent hover:bg-surface-container-low/30 transition-colors">
                <div className="flex justify-between items-center px-2 py-3 mb-1">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-outline-variant"></span>
                    <span className="font-label-md text-[14px] font-bold text-on-surface-variant">Deprecated</span>
                  </div>
                </div>

                <div className="flex items-center justify-center p-xl mt-xl">
                   <p className="font-label-sm text-[12px] text-on-surface-variant italic opacity-60">Empty Archive</p>
                </div>
              </div>

            </div>
          </div>
        </div>

      </div>
    </div>
  );
}
