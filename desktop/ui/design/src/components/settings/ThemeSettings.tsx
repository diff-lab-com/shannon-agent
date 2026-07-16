import React from 'react';
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

export default function ThemeSettings() {
  return (
    <div className="max-w-3xl">
      <header className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">Theme Settings</h2>
        <p className="font-body-md text-on-surface-variant">Customize the visual environment to match your cognitive workflow.</p>
      </header>
      
      <div className="space-y-lg pb-10">
        {/* Appearance Section */}
        <section className="bg-white rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <h3 className="font-headline-md text-headline-md mb-md">Appearance</h3>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-md">
            <label className="cursor-pointer group">
              <input defaultChecked className="hidden peer" name="appearance" type="radio" value="light" />
              <div className="p-md rounded-xl border-2 border-outline-variant/30 peer-checked:border-primary peer-checked:bg-primary-fixed/20 transition-all">
                <div className="aspect-video bg-background rounded-md mb-sm border border-outline-variant/20 overflow-hidden flex items-center justify-center">
                  <span className="material-symbols-outlined text-primary text-display-lg" data-icon="light_mode">light_mode</span>
                </div>
                <p className="text-center font-label-md">Light Mode</p>
              </div>
            </label>
            <label className="cursor-pointer group">
              <input className="hidden peer" name="appearance" type="radio" value="dark" />
              <div className="p-md rounded-xl border-2 border-outline-variant/30 peer-checked:border-primary peer-checked:bg-primary-fixed/20 transition-all bg-inverse-surface/5">
                <div className="aspect-video bg-inverse-surface rounded-md mb-sm border border-outline-variant/20 overflow-hidden flex items-center justify-center">
                  <span className="material-symbols-outlined text-inverse-primary text-display-lg" data-icon="dark_mode">dark_mode</span>
                </div>
                <p className="text-center font-label-md">Dark Mode</p>
              </div>
            </label>
            <label className="cursor-pointer group">
              <input className="hidden peer" name="appearance" type="radio" value="system" />
              <div className="p-md rounded-xl border-2 border-outline-variant/30 peer-checked:border-primary peer-checked:bg-primary-fixed/20 transition-all">
                <div className="aspect-video bg-gradient-to-br from-background to-inverse-surface rounded-md mb-sm border border-outline-variant/20 overflow-hidden flex items-center justify-center">
                  <span className="material-symbols-outlined text-on-surface-variant text-display-lg" data-icon="settings_brightness">settings_brightness</span>
                </div>
                <p className="text-center font-label-md">System</p>
              </div>
            </label>
          </div>
        </section>

        {/* Color Accents Section */}
        <div className="space-y-md pt-md">
          <h3 className="font-headline-md text-headline-md">Color Accents</h3>
          <div className="flex items-center gap-lg">
            <Button className="w-12 h-12 rounded-full bg-[#8B5CF6] ring-offset-4 ring-2 ring-[#8B5CF6] transition-transform active:scale-90 shadow-lg relative flex items-center justify-center cursor-pointer">
              <span className="material-symbols-outlined text-white text-[20px]" data-icon="check">check</span>
            </Button>
            <Button className="w-10 h-10 rounded-full bg-[#3B82F6] hover:scale-110 transition-transform active:scale-90 opacity-60 hover:opacity-100 cursor-pointer"></Button>
            <Button className="w-10 h-10 rounded-full bg-[#14B8A6] hover:scale-110 transition-transform active:scale-90 opacity-60 hover:opacity-100 cursor-pointer"></Button>
            <Button className="w-10 h-10 rounded-full bg-[#F59E0B] hover:scale-110 transition-transform active:scale-90 opacity-60 hover:opacity-100 cursor-pointer"></Button>
            <Button className="w-10 h-10 rounded-full bg-[#EF4444] hover:scale-110 transition-transform active:scale-90 opacity-60 hover:opacity-100 cursor-pointer"></Button>
          </div>
        </div>

        {/* Glass Pane Section */}
        <div className="space-y-md pt-lg">
          <div className="flex items-center justify-between">
            <h3 className="font-headline-md text-headline-md">Glass Pane Intensity</h3>
            <span className="px-md py-xs bg-primary/10 text-primary font-label-md rounded-full border border-primary/20">70% Intensity</span>
          </div>
          <div className="p-lg bg-surface-container-low/50 rounded-2xl border border-outline-variant/20">
            <div className="flex items-center justify-between mb-md px-xs">
              <span className="font-label-sm text-outline">Solid</span>
              <span className="font-label-sm text-outline">Clear</span>
            </div>
            <input className="w-full h-1 bg-outline-variant/40 rounded-lg appearance-none cursor-pointer outline-none slider-thumb-primary" max="100" min="0" type="range" defaultValue="70" />
            <div className="mt-lg p-md glass-surface rounded-xl border border-white/40 shadow-sm flex items-center gap-md bg-white/70 backdrop-blur-md">
              <div className="w-10 h-10 rounded-lg bg-primary/20 flex items-center justify-center">
                <span className="material-symbols-outlined text-primary" data-icon="visibility">visibility</span>
              </div>
              <span className="font-body-sm text-on-surface-variant">Live preview of current transparency level across interface components.</span>
            </div>
          </div>
        </div>

        {/* Interface Font Section */}
        <div className="space-y-md pt-lg">
          <h3 className="font-headline-md text-headline-md">Interface Font</h3>
          <div className="relative group max-w-sm">
            <Select defaultValue="inter">
              <SelectTrigger className="w-full appearance-none bg-white border border-outline-variant/40 rounded-xl px-lg py-md font-body-md focus:ring-2 focus:ring-primary focus:border-primary outline-none transition-all pr-xl cursor-pointer shadow-sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="inter">Inter (Default)</SelectItem>
                <SelectItem value="geist">Geist Sans</SelectItem>
                <SelectItem value="sf-pro">SF Pro Display</SelectItem>
                <SelectItem value="roboto">Roboto Flex</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <p className="font-label-sm text-outline px-sm">Primary typeface used for headlines, body text, and labels.</p>
        </div>
      </div>
    </div>
  );
}
