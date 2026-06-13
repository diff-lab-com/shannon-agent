import React from 'react';
import { Button } from "@/components/ui/button";

export default function BillingSettings() {
  return (
    <div className="pb-xl">
      {/* Page Header */}
      <div className="mb-xl">
        <h2 className="font-headline-lg text-[32px] font-semibold text-on-surface mb-xs">Usage &amp; Billing</h2>
        <p className="font-body-md text-on-surface-variant">Manage your subscription plans, view usage metrics, and update payment information.</p>
      </div>

      <div className="space-y-lg">
        {/* Bento Grid Layout Container */}
        <div className="grid grid-cols-1 md:grid-cols-12 gap-lg">
          
          {/* Section 1: Current Plan */}
          <section className="md:col-span-5 bg-white/70 backdrop-blur-md border border-[#e2e8f0]/80 rounded-2xl p-lg flex flex-col justify-between shadow-sm">
            <div>
              <div className="flex justify-between items-start mb-lg">
                <div>
                  <span className="bg-primary/10 text-primary text-[10px] font-bold px-2 py-1 rounded-full uppercase tracking-wider mb-2 inline-block">Active Plan</span>
                  <h3 className="font-headline-md text-[24px] font-bold">Pro Plan</h3>
                </div>
                <div className="text-right">
                  <p className="font-headline-md text-[24px] font-bold">$29.00</p>
                  <p className="font-label-sm text-label-sm text-on-surface-variant">per month</p>
                </div>
              </div>
              <div className="space-y-4 mb-xl">
                <div className="flex items-center gap-3 text-on-surface-variant">
                  <span className="material-symbols-outlined text-primary">event</span>
                  <span className="font-body-sm text-[14px]">Next renewal: <strong className="text-on-surface">October 12, 2024</strong></span>
                </div>
                <div className="flex items-center gap-3 text-on-surface-variant">
                  <span className="material-symbols-outlined text-primary">credit_card</span>
                  <span className="font-body-sm text-[14px]">Charged to: <strong className="text-on-surface">Visa **** 4242</strong></span>
                </div>
              </div>
            </div>
            <div className="flex gap-3 mt-auto">
              <Button className="flex-1 py-3 px-4 bg-primary text-white rounded-xl font-bold text-center hover:opacity-90 active:scale-[0.98] transition-all cursor-pointer">Change Plan</Button>
              <Button className="px-4 py-3 border border-outline-variant text-on-surface-variant rounded-xl hover:bg-surface-container-low active:scale-[0.98] transition-all cursor-pointer font-bold">Cancel</Button>
            </div>
          </section>

          {/* Section 2: Usage Quota Overview */}
          <section className="md:col-span-7 bg-white/70 backdrop-blur-md border border-[#e2e8f0]/80 rounded-2xl p-lg shadow-sm">
            <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest mb-lg">Usage Quota Overview</h3>
            <div className="grid grid-cols-1 gap-lg md:grid-cols-2">
              
              {/* Token Usage Ring */}
              <div className="flex flex-col items-center text-center">
                <div className="relative w-28 h-28 mb-4 flex items-center justify-center">
                  <svg className="w-full h-full transform -rotate-90">
                    <circle className="text-surface-container-highest" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor" strokeWidth="8"></circle>
                    <circle className="text-primary transition-all duration-1000 ease-out" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor" strokeDasharray="301.6" strokeDashoffset="45.2" strokeWidth="8"></circle>
                  </svg>
                  <div className="absolute flex flex-col items-center">
                    <span className="font-headline-md text-[24px] font-bold">850K</span>
                  </div>
                </div>
                <p className="font-label-md text-[14px] font-bold mb-1">Token Usage</p>
                <p className="font-label-sm text-[12px] text-on-surface-variant">850K</p>
              </div>

              {/* Cache Hit Rate Ring */}
              <div className="flex flex-col items-center text-center">
                <div className="relative w-28 h-28 mb-4 flex items-center justify-center">
                  <svg className="w-full h-full transform -rotate-90">
                    <circle className="text-surface-container-highest" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor" strokeWidth="8"></circle>
                    <circle className="text-secondary transition-all duration-1000 ease-out" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor" strokeDasharray="301.6" strokeDashoffset="96.5" strokeWidth="8"></circle>
                  </svg>
                  <div className="absolute flex flex-col items-center">
                    <span className="font-headline-md text-[24px] font-bold">68%</span>
                  </div>
                </div>
                <p className="font-label-md text-[14px] font-bold mb-1">Cache Hit Rate</p>
                <p className="font-label-sm text-[12px] text-on-surface-variant">Average Cache Hit</p>
              </div>

            </div>
          </section>

          {/* Section 3: Cost Analysis Chart */}
          <section className="md:col-span-12 bg-white/70 backdrop-blur-md border border-[#e2e8f0]/80 rounded-2xl p-lg shadow-sm">
            <div className="flex justify-between items-end mb-xl">
              <div>
                <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest mb-2">Cost Analysis</h3>
                <p className="font-headline-md text-[24px] font-bold">Daily Spending <span className="text-on-surface-variant font-normal text-[14px] ml-1">(Last 30 Days)</span></p>
              </div>
              <div className="flex gap-2">
                <span className="flex items-center gap-2 font-label-md text-[14px] text-on-surface-variant bg-surface-container px-3 py-1 rounded-lg">
                  <span className="w-2 h-2 rounded-full bg-primary"></span>Tokens
                </span>
                <span className="flex items-center gap-2 font-label-md text-[14px] text-on-surface-variant bg-surface-container px-3 py-1 rounded-lg">
                  <span className="w-2 h-2 rounded-full bg-secondary"></span> Cache Hit
                </span>
              </div>
            </div>
            
            <div className="h-48 flex items-end justify-between gap-2 px-2">
              <div className="w-full flex flex-col justify-end h-[40%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[30%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[60%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[40%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[45%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[20%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[70%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[50%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[85%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[25%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[55%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[35%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[40%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[10%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[75%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[40%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[90%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[20%] transition-all duration-1000 ease-out"></div>
              </div>
              <div className="w-full flex flex-col justify-end h-[65%] group relative cursor-pointer hover:brightness-110 transition-all">
                <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                <div className="w-full bg-secondary h-[30%] transition-all duration-1000 ease-out"></div>
              </div>
            </div>
            
            <div className="flex justify-between mt-4 px-2 text-on-surface-variant font-label-sm text-[12px]">
              <span>Sep 01</span>
              <span>Sep 15</span>
              <span>Today</span>
            </div>
          </section>

          {/* Section 4: Billing History Table */}
          <section className="md:col-span-12 bg-white/70 backdrop-blur-md border border-[#e2e8f0]/80 rounded-2xl p-lg overflow-hidden shadow-sm">
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">Billing History</h3>
            </div>
            <div className="overflow-x-auto">
              <table className="w-full text-left">
                <thead>
                  <tr className="border-b border-outline-variant/30 font-label-sm text-[12px] text-on-surface-variant uppercase tracking-wider">
                    <th className="pb-4 font-medium px-2">Date</th>
                    <th className="pb-4 font-medium px-2">Description</th>
                    <th className="pb-4 font-medium px-2 text-right">Amount</th>
                    <th className="pb-4 font-medium px-2 text-center">Status</th>
                  </tr>
                </thead>
                <tbody className="font-body-sm text-[14px]">
                  <tr className="border-b border-outline-variant/10 group hover:bg-surface-container-low transition-colors">
                    <td className="py-4 px-2">Sep 12, 2024</td>
                    <td className="py-4 px-2 font-medium">Pro Plan - Monthly Subscription</td>
                    <td className="py-4 px-2 text-right">$29.00</td>
                    <td className="py-4 px-2 text-center">
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-green-100 text-green-700 text-[11px] font-bold uppercase tracking-wider">
                        <span className="w-1.5 h-1.5 rounded-full bg-green-500"></span> Paid
                      </span>
                    </td>
                  </tr>
                  <tr className="border-b border-outline-variant/10 group hover:bg-surface-container-low transition-colors">
                    <td className="py-4 px-2 text-on-surface-variant">Aug 12, 2024</td>
                    <td className="py-4 px-2 font-medium">Pro Plan - Monthly Subscription</td>
                    <td className="py-4 px-2 text-right">$29.00</td>
                    <td className="py-4 px-2 text-center">
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-green-100 text-green-700 text-[11px] font-bold uppercase tracking-wider">
                        <span className="w-1.5 h-1.5 rounded-full bg-green-500"></span> Paid
                      </span>
                    </td>
                  </tr>
                  <tr className="group hover:bg-surface-container-low transition-colors">
                    <td className="py-4 px-2 text-on-surface-variant">Jul 28, 2024</td>
                    <td className="py-4 px-2 font-medium text-on-surface-variant">Overage Charge - 250k Tokens</td>
                    <td className="py-4 px-2 text-right text-on-surface-variant">$12.50</td>
                    <td className="py-4 px-2 text-center">
                      <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-amber-100 text-amber-700 text-[11px] font-bold uppercase tracking-wider">
                        <span className="w-1.5 h-1.5 rounded-full bg-amber-500"></span> Pending
                      </span>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>

        </div>
      </div>

      {/* Footer Help Section */}
      <footer className="mt-xl flex flex-col md:flex-row justify-between items-center px-lg py-md bg-white/70 backdrop-blur-md border border-[#e2e8f0]/80 rounded-2xl shadow-sm gap-md">
        <div className="flex items-center gap-4 text-center md:text-left">
          <span className="material-symbols-outlined text-primary hidden md:block">info</span>
          <p className="font-body-sm text-[14px] text-on-surface-variant">Need to scale further? Contact our <a className="text-primary font-bold hover:underline cursor-pointer">Enterprise Team</a> for custom quotas.</p>
        </div>
        <div className="flex items-center justify-center gap-6">
          <a className="font-label-sm text-[12px] text-on-surface-variant hover:text-on-surface transition-colors cursor-pointer">Legal &amp; Terms</a>
          <a className="font-label-sm text-[12px] text-on-surface-variant hover:text-on-surface transition-colors cursor-pointer">Privacy Policy</a>
        </div>
      </footer>
    </div>
  );
}
