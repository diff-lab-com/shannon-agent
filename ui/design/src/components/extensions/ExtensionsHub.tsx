import React from 'react';
import { Button } from '@/components/ui/button';

export default function ExtensionsHub() {
  return (
    <div className="max-w-[1200px] mx-auto px-lg pt-lg pb-xl">
      <section className="mb-xl mt-4">
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-headline-md">Available Skills</h3>
          <div className="flex gap-sm">
            <Button variant="ghost" className="px-md py-sm rounded-full bg-surface-container-high font-label-md text-label-md text-on-surface cursor-pointer">Trending</Button>
            <Button variant="ghost" className="px-md py-sm rounded-full font-label-md text-label-md text-on-surface-variant hover:bg-surface-container-high transition-colors cursor-pointer">Recent</Button>
          </div>
        </div>

        {/* Productivity Category */}
        <div className="mb-lg">
          <h4 className="font-label-md text-label-md text-outline uppercase tracking-widest mb-md">Productivity</h4>
          <div className="flex flex-wrap gap-md">
            <div className="group cursor-pointer bg-white border border-outline-variant/50 rounded-xl p-md flex items-center gap-md hover:border-primary transition-all shadow-sm">
              <div className="w-10 h-10 rounded-lg bg-red-100 text-red-600 flex items-center justify-center">
                <span className="material-symbols-outlined">picture_as_pdf</span>
              </div>
              <div>
                <p className="font-label-md text-label-md font-bold">PDF Reader</p>
                <p className="text-label-sm font-label-sm text-on-surface-variant">Token Base: 0.1k</p>
              </div>
              <span className="material-symbols-outlined text-outline group-hover:text-primary ml-sm transition-transform group-hover:scale-125">add_circle</span>
            </div>

            <div className="group cursor-pointer bg-white border border-outline-variant/50 rounded-xl p-md flex items-center gap-md hover:border-primary transition-all shadow-sm">
              <div className="w-10 h-10 rounded-lg bg-blue-100 text-blue-600 flex items-center justify-center">
                <span className="material-symbols-outlined">language</span>
              </div>
              <div>
                <p className="font-label-md text-label-md font-bold">Web Search</p>
                <p className="text-label-sm font-label-sm text-on-surface-variant">Real-time browse</p>
              </div>
              <span className="material-symbols-outlined text-outline group-hover:text-primary ml-sm transition-transform group-hover:scale-125">add_circle</span>
            </div>
          </div>
        </div>

        {/* Design Category */}
        <div className="mb-lg">
          <h4 className="font-label-md text-label-md text-outline uppercase tracking-widest mb-md">Design</h4>
          <div className="flex flex-wrap gap-md">
            <div className="group cursor-pointer bg-white border border-outline-variant/50 rounded-xl p-md flex items-center gap-md hover:border-primary transition-all shadow-sm">
              <div className="w-10 h-10 rounded-lg bg-purple-100 text-purple-600 flex items-center justify-center">
                <span className="material-symbols-outlined">palette</span>
              </div>
              <div>
                <p className="font-label-md text-label-md font-bold">Vector Gen</p>
                <p className="text-label-sm font-label-sm text-on-surface-variant">SVG Creator</p>
              </div>
              <span className="material-symbols-outlined text-outline group-hover:text-primary ml-sm transition-transform group-hover:scale-125">add_circle</span>
            </div>
          </div>
        </div>

        {/* Data & Analysis Category */}
        <div className="mb-lg">
          <h4 className="font-label-md text-label-md text-outline uppercase tracking-widest mb-md">Data &amp; Analysis</h4>
          <div className="flex flex-wrap gap-md">
            <div className="group cursor-pointer bg-white border border-outline-variant/50 rounded-xl p-md flex items-center gap-md hover:border-primary transition-all shadow-sm">
              <div className="w-10 h-10 rounded-lg bg-green-100 text-green-600 flex items-center justify-center">
                <span className="material-symbols-outlined">code</span>
              </div>
              <div>
                <p className="font-label-md text-label-md font-bold">Python Sandbox</p>
                <p className="text-label-sm font-label-sm text-on-surface-variant">Isolated Compute</p>
              </div>
              <span className="material-symbols-outlined text-outline group-hover:text-primary ml-sm transition-transform group-hover:scale-125">add_circle</span>
            </div>
            
            <div className="group cursor-pointer bg-white border border-outline-variant/50 rounded-xl p-md flex items-center gap-md hover:border-primary transition-all shadow-sm">
              <div className="w-10 h-10 rounded-lg bg-orange-100 text-orange-600 flex items-center justify-center">
                <span className="material-symbols-outlined">database</span>
              </div>
              <div>
                <p className="font-label-md text-label-md font-bold">SQL Bridge</p>
                <p className="text-label-sm font-label-sm text-on-surface-variant">Read-only access</p>
              </div>
              <span className="material-symbols-outlined text-outline group-hover:text-primary ml-sm transition-transform group-hover:scale-125">add_circle</span>
            </div>
          </div>
        </div>
      </section>


    </div>
  );
}
