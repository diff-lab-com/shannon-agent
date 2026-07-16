import React from 'react';
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

export default function ModelsSettings() {
  return (
    <div className="max-w-[1200px] pr-8 pb-10">
      <header className="mb-md">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">Model Configuration</h2>
        <p className="font-body-md text-on-surface-variant">Manage your active AI providers and configure default models for your workspace.</p>
      </header>

      <div className="space-y-lg">
        {/* Performance Strategy */}
        <section className="bg-white border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-md">Performance Strategy</h3>
          <Tabs defaultValue="high-quality">
            <TabsList className="flex bg-surface-container-low p-xs rounded-xl gap-xs max-w-2xl">
              <TabsTrigger value="balanced" className="flex-1 py-sm font-label-md rounded-lg text-on-surface-variant hover:bg-surface-container-high transition-all cursor-pointer">Balanced</TabsTrigger>
              <TabsTrigger value="speed" className="flex-1 py-sm font-label-md rounded-lg text-on-surface-variant hover:bg-surface-container-high transition-all cursor-pointer">Speed</TabsTrigger>
              <TabsTrigger value="high-quality" className="flex-1 py-sm font-label-md rounded-lg bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5 transition-all outline-none font-bold">High Quality</TabsTrigger>
            </TabsList>
          </Tabs>
          <p className="mt-md text-label-sm text-on-surface-variant opacity-70 flex items-center gap-xs">
            <span className="material-symbols-outlined text-[16px]">info</span>
            Prioritizes complex reasoning and detailed outputs across all enabled providers.
          </p>
        </section>

        {/* Active Tier Summary */}
        <section className="bg-white border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-md">Active Tier Summary</h3>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-md">
            <div className="p-md bg-surface-container-low rounded-xl border border-outline-variant/30 flex flex-col gap-sm">
              <div className="flex items-center gap-xs">
                <span className="material-symbols-outlined text-primary text-[20px]" style={{fontVariationSettings: "'FILL' 1"}}>diamond</span>
                <span className="font-label-md font-bold text-primary">Pro Tier</span>
              </div>
              <div className="relative">
                <Select defaultValue="gpt4o">
                  <SelectTrigger className="w-full bg-surface-container-lowest border border-outline-variant/50 rounded-lg px-sm py-xs font-body-sm text-on-surface outline-none focus:ring-2 focus:ring-primary appearance-none cursor-pointer">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="gpt4o">GPT-4o</SelectItem>
                    <SelectItem value="claude-3.5-sonnet">Claude 3.5 Sonnet</SelectItem>
                    <SelectItem value="llama-3-70b">Llama 3 (70B)</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="p-md bg-surface-container-low rounded-xl border border-outline-variant/30 flex flex-col gap-sm">
              <div className="flex items-center gap-xs">
                <span className="material-symbols-outlined text-primary text-[20px]" style={{fontVariationSettings: "'FILL' 1"}}>star</span>
                <span className="font-label-md font-bold text-primary">Standard Tier</span>
              </div>
              <div className="relative">
                <Select defaultValue="gpt4t">
                  <SelectTrigger className="w-full bg-surface-container-lowest border border-outline-variant/50 rounded-lg px-sm py-xs font-body-sm text-on-surface outline-none focus:ring-2 focus:ring-primary appearance-none cursor-pointer">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="gpt4t">GPT-4 Turbo</SelectItem>
                    <SelectItem value="claude-3-haiku">Claude 3 Haiku</SelectItem>
                    <SelectItem value="gemini-1.5-pro">Gemini 1.5 Pro</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="p-md bg-surface-container-low rounded-xl border border-outline-variant/30 flex flex-col gap-sm">
              <div className="flex items-center gap-xs">
                <span className="material-symbols-outlined text-primary text-[20px]" style={{fontVariationSettings: "'FILL' 1"}}>bolt</span>
                <span className="font-label-md font-bold text-primary">Lite Tier</span>
              </div>
              <div className="relative">
                <Select defaultValue="gpt35">
                  <SelectTrigger className="w-full bg-surface-container-lowest border border-outline-variant/50 rounded-lg px-sm py-xs font-body-sm text-on-surface outline-none focus:ring-2 focus:ring-primary appearance-none cursor-pointer">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="gpt35">GPT-3.5 Turbo</SelectItem>
                    <SelectItem value="llama-3-8b">Llama 3 (8B)</SelectItem>
                    <SelectItem value="mistral-7b">Mistral 7B</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
          </div>
        </section>

        {/* OpenAI Models Config */}
        <section className="bg-white border border-outline-variant/30 rounded-xl shadow-sm overflow-hidden">
          <div className="border-b border-outline-variant/30 bg-surface-container-low/30 px-lg pt-md">
            <Tabs defaultValue="openai">
              <TabsList className="flex gap-lg overflow-x-auto custom-scrollbar">
                <TabsTrigger value="openai" className="pb-sm px-xs border-b-2 border-primary text-primary font-bold font-label-md whitespace-nowrap outline-none">OpenAI</TabsTrigger>
                <TabsTrigger value="anthropic" className="pb-sm px-xs border-b-2 border-transparent text-on-surface-variant font-label-md hover:text-on-surface transition-colors whitespace-nowrap cursor-pointer">Anthropic</TabsTrigger>
                <TabsTrigger value="google" className="pb-sm px-xs border-b-2 border-transparent text-on-surface-variant font-label-md hover:text-on-surface transition-colors whitespace-nowrap cursor-pointer">Google</TabsTrigger>
                <TabsTrigger value="meta" className="pb-sm px-xs border-b-2 border-transparent text-on-surface-variant font-label-md hover:text-on-surface transition-colors whitespace-nowrap cursor-pointer">Meta</TabsTrigger>
              </TabsList>
            </Tabs>
          </div>
          
          <div className="p-lg">
            <div className="flex justify-between items-center mb-lg">
              <div>
                <h3 className="font-headline-md text-on-surface">OpenAI Models</h3>
                <p className="text-body-sm text-on-surface-variant">Select active models and set your global default.</p>
              </div>
              <div className="flex gap-sm">
                <span className="inline-flex items-center px-sm py-1 bg-green-100 text-green-700 rounded-full text-[10px] font-bold tracking-wider uppercase">Connection Active</span>
              </div>
            </div>
            
            <div className="grid grid-cols-1 gap-md">
              {/* GPT-4o */}
              <div className="p-md rounded-xl border-2 border-primary bg-primary-container/5 flex items-center justify-between transition-all group">
                <div className="flex items-center gap-md">
                  <div className="w-10 h-10 rounded-lg bg-primary text-on-primary flex items-center justify-center">
                    <span className="material-symbols-outlined">auto_awesome</span>
                  </div>
                  <div>
                    <div className="flex items-center gap-xs">
                      <span className="font-headline-md text-primary text-lg">GPT-4o</span>
                      <span className="px-xs py-[2px] bg-primary text-on-primary rounded text-[10px] font-bold">DEFAULT</span>
                    </div>
                    <p className="text-label-sm text-on-surface-variant opacity-70">Multimodal intelligence, high reasoning speed.</p>
                  </div>
                </div>
                <div className="text-right mr-sm">
                  <span className="block text-label-sm font-bold text-primary">Tier: Pro</span>
                  <span className="text-[10px] text-on-surface-variant">Active in 12 agents</span>
                </div>
              </div>

              {/* GPT-4 Turbo */}
              <div className="p-md rounded-xl border border-outline-variant/50 flex items-center justify-between hover:border-primary/50 transition-all group cursor-pointer">
                <div className="flex items-center gap-md">
                  <div className="w-10 h-10 rounded-lg bg-surface-container-high text-on-surface-variant flex items-center justify-center">
                    <span className="material-symbols-outlined">psychology</span>
                  </div>
                  <div>
                    <span className="font-headline-md text-on-surface text-lg">GPT-4 Turbo</span>
                    <p className="text-label-sm text-on-surface-variant opacity-70">Proven performance for long-context tasks.</p>
                  </div>
                </div>
              </div>

              {/* GPT-3.5 Turbo */}
              <div className="p-md rounded-xl border border-outline-variant/50 flex items-center justify-between hover:border-primary/50 transition-all group cursor-pointer">
                <div className="flex items-center gap-md">
                  <div className="w-10 h-10 rounded-lg bg-surface-container-high text-on-surface-variant flex items-center justify-center">
                    <span className="material-symbols-outlined">bolt</span>
                  </div>
                  <div>
                    <span className="font-headline-md text-on-surface text-lg">GPT-3.5 Turbo</span>
                    <p className="text-label-sm text-on-surface-variant opacity-70">Fastest response times for simple automation.</p>
                  </div>
                </div>
              </div>
            </div>
          </div>
          
          <div className="bg-surface-container-low/50 p-lg border-t border-outline-variant/30">
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-primary">key</span>
              <h4 className="font-label-md font-bold text-on-surface">OpenAI API Connection</h4>
            </div>
            <div className="flex gap-md max-w-xl">
              <div className="relative flex-1">
                <Input className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none transition-all font-body-sm" type="password" defaultValue="sk-••••••••••••••••••••••••" />
                <Button variant="ghost" className="absolute right-md top-1/2 -translate-y-1/2 text-on-surface-variant hover:text-primary cursor-pointer">
                  <span className="material-symbols-outlined text-[20px]">visibility</span>
                </Button>
              </div>
              <Button className="px-lg py-sm border border-outline-variant bg-white text-on-surface font-label-md rounded-lg hover:bg-surface-container transition-colors flex items-center gap-sm whitespace-nowrap cursor-pointer">
                <span className="material-symbols-outlined text-[18px]">sync</span>
                Test Connection
              </Button>
            </div>
          </div>
        </section>

        {/* Global Parameters */}
        <section className="bg-white border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-lg">Global Parameters</h3>
          <p className="text-body-sm text-on-surface-variant mb-xl -mt-md">These settings apply to the default model unless overridden at the agent level.</p>
          
          <div className="space-y-xl max-w-2xl">
            <div>
              <div className="flex justify-between items-center mb-sm">
                <label className="font-label-md text-on-surface-variant">Temperature</label>
                <span className="font-label-sm text-primary bg-primary-container/20 px-sm py-xs rounded">0.7</span>
              </div>
              <input className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary" max="1" min="0" step="0.1" type="range" defaultValue="0.7" />
              <div className="flex justify-between mt-xs">
                <span className="text-label-sm text-on-surface-variant/50">Precise</span>
                <span className="text-label-sm text-on-surface-variant/50">Creative</span>
              </div>
            </div>
            
            <div>
              <div className="flex justify-between items-center mb-sm">
                <label className="font-label-md text-on-surface-variant">Max Tokens</label>
                <span className="font-label-sm text-primary bg-primary-container/20 px-sm py-xs rounded">4096</span>
              </div>
              <input className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary" max="128000" min="256" step="256" type="range" defaultValue="4096" />
              <div className="flex justify-between mt-xs">
                <span className="text-label-sm text-on-surface-variant/50">Short</span>
                <span className="text-label-sm text-on-surface-variant/50">Long Context</span>
              </div>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
