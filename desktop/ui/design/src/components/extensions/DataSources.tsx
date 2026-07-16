import React from 'react';
import { Button } from '@/components/ui/button';

export default function DataSources() {
  return (
    <div className="max-w-[1200px] mx-auto px-lg py-xl">
      <div className="mb-lg">
        <h2 className="text-headline-lg font-headline-lg text-on-surface">Data Sources</h2>
        <p className="text-body-md text-on-surface-variant max-w-2xl">Manage the connected knowledge bases your agents use to provide context-aware intelligence. Connect, sync, and index new data silos effortlessly.</p>
      </div>

      <div className="grid grid-cols-12 gap-lg pb-10">
        {/* Featured: Google Drive */}
        <div className="col-span-12 lg:col-span-8 bg-surface-container-lowest border border-outline-variant/50 rounded-xl p-lg shadow-sm relative overflow-hidden group">
          <div className="flex justify-between items-start mb-xl relative z-10">
            <div className="flex items-center gap-md">
              <div className="w-16 h-16 rounded-xl bg-blue-50 flex items-center justify-center border border-blue-100">
                <span className="material-symbols-outlined text-blue-600 text-[32px]" data-icon="cloud" style={{fontVariationSettings: "'FILL' 1"}}>cloud</span>
              </div>
              <div>
                <h3 className="text-headline-md font-headline-md text-on-surface">Google Drive</h3>
                <div className="flex items-center gap-sm mt-xs">
                  <span className="px-sm py-[2px] bg-emerald-50 text-emerald-700 rounded-full text-label-sm font-label-sm flex items-center gap-xs border border-emerald-200">
                    <span className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse"></span>
                    Synced
                  </span>
                  <span className="text-label-sm font-label-sm text-on-surface-variant">Last update: 12 minutes ago</span>
                </div>
              </div>
            </div>
            <Button variant="outline" className="px-md py-sm border border-outline-variant rounded-lg font-label-md text-label-md hover:bg-surface-variant/50 transition-all cursor-pointer">Configure</Button>
          </div>
          
          <div className="grid grid-cols-1 md:grid-cols-3 gap-lg relative z-10">
            <div className="p-md rounded-xl bg-surface-container-low border border-outline-variant/30">
              <p className="text-label-sm font-label-sm text-on-surface-variant uppercase tracking-wider mb-sm">Files Indexed</p>
              <p className="text-headline-md font-headline-md text-on-surface">1,284</p>
              <div className="w-full bg-surface-container-high h-1 rounded-full mt-md overflow-hidden">
                <div className="bg-primary h-full w-3/4 rounded-full"></div>
              </div>
            </div>
            
            <div className="p-md rounded-xl bg-surface-container-low border border-outline-variant/30">
              <p className="text-label-sm font-label-sm text-on-surface-variant uppercase tracking-wider mb-sm">Storage Used</p>
              <p className="text-headline-md font-headline-md text-on-surface">4.2 <span className="text-body-md text-on-surface-variant">GB</span></p>
              <p className="text-label-sm font-label-sm text-on-surface-variant mt-md">84% of allocated buffer</p>
            </div>
            
            <div className="p-md rounded-xl bg-surface-container-low border border-outline-variant/30">
              <p className="text-label-sm font-label-sm text-on-surface-variant uppercase tracking-wider mb-sm">Permissions</p>
              <div className="flex -space-x-2 mt-sm items-center">
                <div className="w-8 h-8 rounded-full bg-surface-container-highest border-2 border-surface-container-low flex items-center justify-center">
                  <span className="material-symbols-outlined text-[16px] text-on-surface-variant">person</span>
                </div>
                <div className="w-8 h-8 rounded-full bg-surface-container-highest border-2 border-surface-container-low flex items-center justify-center">
                  <span className="material-symbols-outlined text-[16px] text-on-surface-variant">person</span>
                </div>
                <div className="w-8 h-8 rounded-full bg-surface-container-high border-2 border-surface-container-low flex items-center justify-center text-[10px] font-bold text-on-surface-variant">+5</div>
              </div>
              <p className="text-label-sm font-label-sm text-primary mt-md cursor-pointer hover:underline">Manage Access</p>
            </div>
          </div>
        </div>

        {/* AI Insight Pipeline */}
        <div className="col-span-12 lg:col-span-4 flex flex-col gap-lg">
          <div className="bg-white border border-outline-variant/50 rounded-xl p-lg shadow-sm flex-1">
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-primary" data-icon="bolt">bolt</span>
              <h3 className="font-label-md text-label-md font-bold text-on-surface">AI Insight Pipeline</h3>
            </div>
            <div className="space-y-md">
              <div className="flex gap-sm items-start">
                <div className="flex flex-col items-center mt-1">
                  <span className="w-6 h-6 rounded-full bg-primary/10 flex items-center justify-center">
                    <span className="w-2 h-2 rounded-full bg-primary animate-pulse"></span>
                  </span>
                  <div className="thought-connector"></div>
                </div>
                <div className="flex-1">
                  <p className="text-label-md font-label-md text-on-surface font-medium">Analyzing Q4 Report PDF</p>
                  <p className="text-label-sm font-label-sm text-on-surface-variant">Extracting financial metrics...</p>
                </div>
              </div>
              
              <div className="flex gap-sm items-start">
                <div className="flex flex-col items-center mt-1">
                  <span className="w-6 h-6 rounded-full bg-surface-container-high flex items-center justify-center">
                    <span className="material-symbols-outlined text-[14px] text-on-surface-variant" data-icon="check">check</span>
                  </span>
                  <div className="thought-connector opacity-30"></div>
                </div>
                <div className="flex-1">
                  <p className="text-label-md font-label-md text-on-surface-variant">Synced Notion Database</p>
                  <p className="text-label-sm font-label-sm text-on-surface-variant opacity-70">214 new entries indexed</p>
                </div>
              </div>
              
              <div className="flex gap-sm items-start">
                <div className="flex flex-col items-center mt-1">
                  <span className="w-6 h-6 rounded-full bg-surface-container-high flex items-center justify-center">
                    <span className="material-symbols-outlined text-[14px] text-on-surface-variant" data-icon="hourglass_empty">hourglass_empty</span>
                  </span>
                </div>
                <div className="flex-1">
                  <p className="text-label-md font-label-md text-on-surface-variant">Pending: Slack Archives</p>
                  <p className="text-label-sm font-label-sm text-on-surface-variant opacity-70">Waiting for rate limit reset</p>
                </div>
              </div>
            </div>
            <Button variant="outline" className="w-full mt-lg py-sm bg-surface-container text-on-surface-variant rounded-lg font-label-md text-label-md hover:bg-surface-container-high transition-colors cursor-pointer border border-outline-variant/30">View Real-time Logs</Button>
          </div>
        </div>

        {/* Secondary Sources */}
        <div className="col-span-12 md:col-span-6 lg:col-span-4 bg-white border border-outline-variant/50 rounded-xl p-md shadow-sm hover:shadow-md transition-shadow cursor-pointer">
          <div className="flex items-center justify-between mb-md">
            <div className="flex items-center gap-md">
              <div className="w-10 h-10 rounded-lg bg-on-background flex items-center justify-center text-white">
                <span className="material-symbols-outlined" data-icon="sticky_note_2">sticky_note_2</span>
              </div>
              <div>
                <h4 className="font-label-md text-label-md font-bold text-on-surface">Notion Workspace</h4>
                <p className="text-label-sm font-label-sm text-on-surface-variant">Personal &amp; Engineering</p>
              </div>
            </div>
            <span className="material-symbols-outlined text-on-surface-variant" data-icon="more_vert">more_vert</span>
          </div>
          <div className="flex items-center justify-between pt-sm border-t border-outline-variant/30">
            <span className="text-label-sm font-label-sm text-on-surface-variant">42 Pages indexed</span>
            <span className="text-label-sm font-label-sm text-emerald-600 font-bold">Healthy</span>
          </div>
        </div>
        
        <div className="col-span-12 md:col-span-6 lg:col-span-4 bg-white border border-error/20 rounded-xl p-md shadow-sm hover:shadow-md transition-shadow cursor-pointer">
          <div className="flex items-center justify-between mb-md">
            <div className="flex items-center gap-md">
              <div className="w-10 h-10 rounded-lg bg-blue-100 flex items-center justify-center text-blue-800">
                <span className="material-symbols-outlined" data-icon="database">database</span>
              </div>
              <div>
                <h4 className="font-label-md text-label-md font-bold text-on-surface">MySQL Analytics</h4>
                <p className="text-label-sm font-label-sm text-on-surface-variant">Production Read-only</p>
              </div>
            </div>
            <span className="material-symbols-outlined text-error" data-icon="warning">warning</span>
          </div>
          <div className="flex items-center justify-between pt-sm border-t border-outline-variant/30">
            <span className="text-label-sm font-label-sm text-on-surface-variant">Connection Timeout</span>
            <span className="text-label-sm font-label-sm text-error font-bold underline cursor-pointer">Reconnect</span>
          </div>
        </div>

        {/* Quick Connect */}
        <div className="col-span-12 lg:col-span-4 bg-surface-container-low/50 border border-dashed border-outline-variant rounded-xl p-md flex flex-col justify-center items-center gap-md min-h-[140px] group hover:border-primary/50 transition-colors">
          <p className="font-label-md text-label-md font-medium text-on-surface-variant">Add New Source</p>
          <div className="flex gap-md">
            <div className="w-10 h-10 rounded-full bg-white border border-outline-variant flex items-center justify-center text-on-surface-variant hover:text-primary hover:border-primary cursor-pointer transition-all active:scale-95 shadow-sm" title="Local Files">
              <span className="material-symbols-outlined" data-icon="upload_file">upload_file</span>
            </div>
            <div className="w-10 h-10 rounded-full bg-white border border-outline-variant flex items-center justify-center text-on-surface-variant hover:text-primary hover:border-primary cursor-pointer transition-all active:scale-95 shadow-sm" title="GitHub">
              <span className="material-symbols-outlined" data-icon="terminal">terminal</span>
            </div>
            <div className="w-10 h-10 rounded-full bg-white border border-outline-variant flex items-center justify-center text-on-surface-variant hover:text-primary hover:border-primary cursor-pointer transition-all active:scale-95 shadow-sm" title="Slack">
              <span className="material-symbols-outlined" data-icon="forum">forum</span>
            </div>
            <div className="w-10 h-10 rounded-full bg-surface-container border border-outline-variant border-dashed flex items-center justify-center text-on-surface-variant hover:text-primary hover:border-primary cursor-pointer transition-all active:scale-95" title="More">
              <span className="material-symbols-outlined" data-icon="add">add</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
