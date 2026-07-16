import React from 'react';

export default function GeneralSettings() {
  return (
    <div className="max-w-3xl">
      <header className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">System Settings</h2>
        <p className="font-body-md text-on-surface-variant">Refine your AI workflow and interface preferences.</p>
      </header>

      <div className="space-y-lg">
        {/* Accessibility */}
        <section className="bg-white rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <h3 className="font-headline-md text-headline-md mb-md">Accessibility</h3>
          <div className="space-y-xl">
            <div className="space-y-sm">
              <div className="flex justify-between items-center">
                <label className="font-label-md text-on-surface">Text Size</label>
                <span className="text-primary font-label-sm bg-primary-fixed px-sm py-xs rounded">Standard</span>
              </div>
              <input className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary" max="4" min="1" type="range" defaultValue="2" />
              <div className="flex justify-between font-label-sm text-outline">
                <span>Compact</span>
                <span>Standard</span>
                <span>Medium</span>
                <span>Large</span>
              </div>
            </div>
          </div>
        </section>

        {/* Autonomy Level */}
        <section className="bg-white rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>auto_awesome</span>
            <h3 className="font-headline-md text-headline-md">Autonomy Level</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">Control the degree of independent decision-making permitted for your active agents. Progressive disclosure ensures you remain in the loop.</p>
          <div className="space-y-sm">
            <input className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary" max="100" min="0" type="range" defaultValue="45" />
            <div className="flex justify-between font-label-sm text-outline px-1">
              <div className="text-left">
                <p className="font-bold text-on-surface">Human-in-the-loop</p>
                <p>High supervision</p>
              </div>
              <div className="text-center">
                <p className="font-bold text-on-surface">Hybrid</p>
                <p>Shared context</p>
              </div>
              <div className="text-right">
                <p className="font-bold text-on-surface">Full Autonomy</p>
                <p>Result focused</p>
              </div>
            </div>
          </div>
        </section>
      </div>

      <style>{`
        input.slider-thumb-primary::-webkit-slider-thumb {
            -webkit-appearance: none;
            height: 16px;
            width: 16px;
            border-radius: 50%;
            background: var(--color-primary);
            cursor: pointer;
            box-shadow: 0 0 10px rgba(107, 56, 212, 0.3);
            margin-top: -6px;
        }
      `}</style>
    </div>
  );
}
