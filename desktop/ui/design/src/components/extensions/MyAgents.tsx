import React from 'react';
import { Button } from '@/components/ui/button';

export default function MyAgents() {
  return (
    <div className="max-w-[1200px] mx-auto px-lg py-xl">
      {/* Page Header & Actions */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg mb-xl">
        <div>
          <h2 className="text-headline-lg font-headline-lg text-on-surface">My Agents</h2>
          <p className="text-body-md text-on-surface-variant">Manage and monitor your deployed autonomous intelligence units.</p>
        </div>
        <div className="flex items-center gap-md">
          <Button variant="outline" className="flex items-center gap-sm border border-outline-variant px-lg py-sm rounded-xl font-bold text-label-md hover:bg-surface-variant/50 transition-colors cursor-pointer">
            <span className="material-symbols-outlined" data-icon="upload">upload</span>
            Import Agent
          </Button>
        </div>
      </div>

      {/* Agents Bento Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-lg">
        {/* Agent Card 1: Researcher */}
        <div className="glass-card p-lg rounded-xl shadow-sm flex flex-col group hover:-translate-y-1 transition-transform duration-300">
          <div className="flex justify-between items-start mb-md">
            <div className="w-12 h-12 rounded-xl bg-primary-container/20 flex items-center justify-center text-primary">
              <span className="material-symbols-outlined text-[28px]" data-icon="query_stats" style={{fontVariationSettings: "'FILL' 1"}}>query_stats</span>
            </div>
            <div className="flex items-center gap-xs px-sm py-1 bg-green-100 text-green-700 rounded-full">
              <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse"></span>
              <span className="text-label-sm">Active</span>
            </div>
          </div>
          
          <div className="mb-lg">
            <h3 className="text-headline-md font-headline-md">Researcher</h3>
            <p className="text-label-sm text-on-surface-variant">v2.4.1 • Autonomous Intelligence</p>
          </div>
          
          <div className="space-y-sm mb-lg">
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Data Scope</span>
              <span className="font-bold flex items-center gap-xs">
                <span className="material-symbols-outlined text-sm" data-icon="public">public</span> Web, ArXiv
              </span>
            </div>
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Cost per Request</span>
              <span className="font-bold">$0.02</span>
            </div>
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Total Tasks</span>
              <span className="font-bold">1,284</span>
            </div>
          </div>
          
          <div className="mt-auto pt-md border-t border-outline-variant flex gap-sm">
            <Button variant="ghost" className="flex-grow py-2 rounded-lg bg-surface-variant/50 font-bold text-label-md hover:bg-surface-variant transition-colors cursor-pointer">Configure</Button>
            <Button variant="ghost" className="p-2 rounded-lg border border-outline-variant hover:text-primary transition-colors cursor-pointer flex items-center justify-center">
              <span className="material-symbols-outlined" data-icon="more_horiz">more_horiz</span>
            </Button>
          </div>
        </div>

        {/* Agent Card 2: AutoCoder */}
        <div className="glass-card p-lg rounded-xl shadow-sm flex flex-col group hover:-translate-y-1 transition-transform duration-300">
          <div className="flex justify-between items-start mb-md">
            <div className="w-12 h-12 rounded-xl bg-secondary-container/20 flex items-center justify-center text-secondary">
              <span className="material-symbols-outlined text-[28px]" data-icon="code" style={{fontVariationSettings: "'FILL' 1"}}>code</span>
            </div>
            <div className="flex items-center gap-xs px-sm py-1 bg-surface-container-high text-on-surface-variant rounded-full">
              <span className="w-2 h-2 rounded-full bg-outline"></span>
              <span className="text-label-sm">Idle</span>
            </div>
          </div>
          
          <div className="mb-lg">
            <h3 className="text-headline-md font-headline-md">AutoCoder</h3>
            <p className="text-label-sm text-on-surface-variant">v1.1.0 • Technical Architecture</p>
          </div>
          
          <div className="space-y-sm mb-lg">
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Data Scope</span>
              <span className="font-bold flex items-center gap-xs">
                <span className="material-symbols-outlined text-sm" data-icon="terminal">terminal</span> GitHub, Jira
              </span>
            </div>
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Cost per Request</span>
              <span className="font-bold">$0.08</span>
            </div>
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Total Tasks</span>
              <span className="font-bold">412</span>
            </div>
          </div>
          
          <div className="mt-auto pt-md border-t border-outline-variant flex gap-sm">
            <Button variant="ghost" className="flex-grow py-2 rounded-lg bg-primary text-on-primary font-bold text-label-md hover:opacity-90 transition-opacity cursor-pointer">Deploy Now</Button>
            <Button variant="ghost" className="p-2 rounded-lg border border-outline-variant hover:text-primary transition-colors cursor-pointer flex items-center justify-center">
              <span className="material-symbols-outlined" data-icon="settings">settings</span>
            </Button>
          </div>
        </div>

        {/* Agent Card 3: PA Agent */}
        <div className="glass-card p-lg rounded-xl shadow-sm flex flex-col group hover:-translate-y-1 transition-transform duration-300">
          <div className="flex justify-between items-start mb-md">
            <div className="w-12 h-12 rounded-xl bg-tertiary-container/20 flex items-center justify-center text-tertiary">
              <span className="material-symbols-outlined text-[28px]" data-icon="schedule" style={{fontVariationSettings: "'FILL' 1"}}>schedule</span>
            </div>
            <div className="flex items-center gap-xs px-sm py-1 bg-green-100 text-green-700 rounded-full">
              <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse"></span>
              <span className="text-label-sm">Active</span>
            </div>
          </div>
          
          <div className="mb-lg">
            <h3 className="text-headline-md font-headline-md">PA Agent</h3>
            <p className="text-label-sm text-on-surface-variant">v3.0.5 • Logistics &amp; Planning</p>
          </div>
          
          <div className="space-y-sm mb-lg">
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Data Scope</span>
              <span className="font-bold flex items-center gap-xs">
                <span className="material-symbols-outlined text-sm" data-icon="description">description</span> Email, PDFs
              </span>
            </div>
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Cost per Request</span>
              <span className="font-bold">$0.01</span>
            </div>
            <div className="flex justify-between items-center text-label-md">
              <span className="text-on-surface-variant">Total Tasks</span>
              <span className="font-bold">2,910</span>
            </div>
          </div>
          
          <div className="mt-auto pt-md border-t border-outline-variant flex gap-sm">
            <Button variant="ghost" className="flex-grow py-2 rounded-lg bg-surface-variant/50 font-bold text-label-md hover:bg-surface-variant transition-colors cursor-pointer">Configure</Button>
            <Button variant="ghost" className="p-2 rounded-lg border border-outline-variant hover:text-primary transition-colors cursor-pointer flex items-center justify-center">
              <span className="material-symbols-outlined" data-icon="analytics">analytics</span>
            </Button>
          </div>
        </div>

        {/* Empty State / Add New */}
        <div className="border-2 border-dashed border-outline-variant p-lg rounded-xl flex flex-col items-center justify-center text-center group cursor-pointer hover:border-primary/50 transition-colors">
          <div className="w-12 h-12 rounded-full bg-surface-container flex items-center justify-center text-on-surface-variant group-hover:bg-primary-container/20 group-hover:text-primary transition-colors mb-md">
            <span className="material-symbols-outlined text-[32px]" data-icon="add">add</span>
          </div>
          <h3 className="text-body-lg font-bold">New Specialization</h3>
          <p className="text-label-md text-on-surface-variant max-w-[200px]">Define a custom prompt or import a model to create a new agent.</p>
        </div>
      </div>

      {/* Detailed Insight Section */}
      <section className="mt-xl grid grid-cols-1 lg:grid-cols-3 gap-lg mb-8">
        <div className="lg:col-span-2 glass-card p-xl rounded-xl">
          <h4 className="text-body-lg font-bold mb-lg flex items-center gap-md">
            <span className="material-symbols-outlined text-primary" data-icon="insights">insights</span>
            Cognitive Processing Performance
          </h4>
          
          <div className="relative h-48 w-full bg-surface-container-low rounded-lg overflow-hidden flex items-end px-lg pb-lg gap-md pt-10">
            {/* Mock Bar Chart */}
            <div className="flex-grow bg-primary/20 h-[40%] rounded-t-sm relative group">
              <div className="absolute -top-10 left-1/2 -translate-x-1/2 bg-on-surface text-surface text-[10px] px-2 py-1 rounded opacity-0 group-hover:opacity-100 transition-opacity">Researcher</div>
            </div>
            <div className="flex-grow bg-primary/40 h-[65%] rounded-t-sm relative group">
              <div className="absolute -top-10 left-1/2 -translate-x-1/2 bg-on-surface text-surface text-[10px] px-2 py-1 rounded opacity-0 group-hover:opacity-100 transition-opacity">AutoCoder</div>
            </div>
            <div className="flex-grow bg-primary/30 h-[90%] rounded-t-sm relative group">
              <div className="absolute -top-10 left-1/2 -translate-x-1/2 bg-on-surface text-surface text-[10px] px-2 py-1 rounded opacity-0 group-hover:opacity-100 transition-opacity">PA Agent</div>
            </div>
            <div className="flex-grow bg-primary/10 h-[25%] rounded-t-sm relative group"></div>
            <div className="flex-grow bg-primary/15 h-[55%] rounded-t-sm relative group"></div>
            <div className="flex-grow bg-primary/25 h-[75%] rounded-t-sm relative group"></div>
          </div>
          
          <div className="flex justify-between mt-md text-label-sm text-on-surface-variant">
            <span>Mon</span><span>Tue</span><span>Wed</span><span>Thu</span><span>Fri</span><span>Sat</span><span>Sun</span>
          </div>
        </div>
        
        <div className="glass-card p-xl rounded-xl">
          <h4 className="text-body-lg font-bold mb-lg">Active Data Scopes</h4>
          <div className="space-y-md">
            <div className="flex items-center gap-md">
              <div className="w-2 h-8 rounded-full bg-primary"></div>
              <div className="flex-grow">
                <p className="text-label-md font-bold">Vector Database (SQL)</p>
                <div className="w-full bg-surface-container-high h-1.5 rounded-full mt-1">
                  <div className="bg-primary w-[85%] h-full rounded-full"></div>
                </div>
              </div>
              <span className="text-label-sm font-bold">85%</span>
            </div>
            
            <div className="flex items-center gap-md">
              <div className="w-2 h-8 rounded-full bg-secondary"></div>
              <div className="flex-grow">
                <p className="text-label-md font-bold">Web Documentation</p>
                <div className="w-full bg-surface-container-high h-1.5 rounded-full mt-1">
                  <div className="bg-secondary w-[42%] h-full rounded-full"></div>
                </div>
              </div>
              <span className="text-label-sm font-bold">42%</span>
            </div>
            
            <div className="flex items-center gap-md">
              <div className="w-2 h-8 rounded-full bg-tertiary"></div>
              <div className="flex-grow">
                <p className="text-label-md font-bold">Local File Clusters</p>
                <div className="w-full bg-surface-container-high h-1.5 rounded-full mt-1">
                  <div className="bg-tertiary w-[12%] h-full rounded-full"></div>
                </div>
              </div>
              <span className="text-label-sm font-bold">12%</span>
            </div>
          </div>
          <Button variant="ghost" className="w-full mt-lg text-primary text-label-md font-bold hover:underline cursor-pointer text-left">Manage All Scopes →</Button>
        </div>
      </section>
    </div>
  );
}
