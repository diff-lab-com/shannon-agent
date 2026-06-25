import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { useApp } from '@/context/AppContext'
import { CardSkeleton } from '@/components/SkeletonLoader'
import * as api from '@/lib/tauri-api'
import type { BillingPlan, CostRecord, BillingHistory } from '@/types'

export default function BillingSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { usage, status } = useApp()
  const [plan, setPlan] = useState<BillingPlan | null>(null)
  const [costHistory, setCostHistory] = useState<CostRecord[]>([])
  const [billingHistory, setBillingHistory] = useState<BillingHistory[]>([])
  const [loading, setLoading] = useState(true)
  const [showChangePlan, setShowChangePlan] = useState(false)
  const [showLegal, setShowLegal] = useState(false)
  const [showCancelConfirm, setShowCancelConfirm] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [changingPlan, setChangingPlan] = useState<string | null>(null)

  const handleCancelSubscription = async () => {
    setCancelling(true)
    try {
      await api.configure({ key: 'cancel_subscription', value: 'true' })
      toast.success(t('settings.billing.cancelSuccess'))
      setShowCancelConfirm(false)
    } catch (e) { console.warn("BillingSettings cancel error:", e); toast.error(t('settings.billing.cancelFailed')) }
    setCancelling(false)
  }

  const handleChangePlan = async (planName: string) => {
    setChangingPlan(planName)
    try {
      await api.configure({ key: 'plan', value: planName.toLowerCase() })
      toast.success(intl.formatMessage({ id: 'settings.billing.planSwitched' }, { plan: planName }))
      setShowChangePlan(false)
    } catch (e) { console.warn("BillingSettings plan error:", e); toast.error(t('settings.billing.planChangeFailed')) }
    setChangingPlan(null)
  }

  useEffect(() => {
    Promise.all([
      api.getBillingPlan().then(setPlan).catch(() => toast.error(t('settings.billing.loadPlanFailed'))),
      api.getCostHistory(30).then(setCostHistory).catch(() => toast.error(t('settings.billing.loadCostFailed'))),
      api.getBillingHistory().then(setBillingHistory).catch(() => toast.error(t('settings.billing.loadHistoryFailed'))),
    ]).finally(() => setLoading(false))
  }, [])

  const inputTokens = usage?.input_tokens ?? 0
  const outputTokens = usage?.output_tokens ?? 0
  const totalTokens = inputTokens + outputTokens
  const cacheHitRate = usage?.cache_hit_rate ?? 0
  const maxCost = Math.max(...costHistory.map(c => c.cost_usd), 0.01)

  return (
    <div className="pb-xl">
      {/* Page Header */}
      <div className="mb-xl">
        <h2 className="font-headline-lg text-[32px] font-semibold text-on-surface mb-xs">{t('settings.billing.title')}</h2>
        <p className="font-body-md text-on-surface-variant">{t('settings.billing.subtitle')}</p>
      </div>

      {/* Demo mode banner */}
      <div
        role="alert"
        data-testid="demo-banner"
        className="mb-lg flex items-start gap-md p-md rounded-xl bg-warning/10 border border-warning/30 text-on-surface"
      >
        <span className="material-symbols-outlined text-warning shrink-0">science</span>
        <div className="flex-1">
          <div className="font-label-md text-warning font-bold">{t('settings.billing.demoMode')}</div>
          <p className="font-body-sm text-on-surface-variant mt-xs">
            {t('settings.billing.demoDesc')}
          </p>
          <p className="font-body-sm text-on-surface-variant mt-xs">
            {t('settings.billing.demoActionsDisabled')}
          </p>
        </div>
      </div>

      <div className="space-y-lg">
        {loading ? (
          <div className="grid grid-cols-1 md:grid-cols-12 gap-lg">
            <div className="md:col-span-5"><CardSkeleton /></div>
            <div className="md:col-span-7"><CardSkeleton /></div>
            <div className="md:col-span-12"><CardSkeleton /></div>
          </div>
        ) : (
        <div className="grid grid-cols-1 md:grid-cols-12 gap-lg">

          {/* Section 1: Current Plan */}
          <section className="md:col-span-5 bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/30 rounded-2xl p-lg flex flex-col justify-between shadow-sm">
            <div>
              <div className="flex justify-between items-start mb-lg">
                <div>
                  <span className="bg-primary/10 text-primary text-[10px] font-bold px-2 py-1 rounded-full uppercase tracking-wider mb-2 inline-block">{t('settings.billing.activePlan')}</span>
                  <h3 className="font-headline-md text-[24px] font-bold">{intl.formatMessage({ id: 'settings.billing.planName' }, { name: plan?.name ?? 'Pro' })}</h3>
                </div>
                <div className="text-right">
                  <p className="font-headline-md text-[24px] font-bold">${plan?.price ?? 29}.00</p>
                  <p className="font-label-sm text-label-sm text-on-surface-variant">{t('settings.billing.perMonth')}</p>
                </div>
              </div>
              <div className="space-y-4 mb-xl">
                <div className="flex items-center gap-3 text-on-surface-variant">
                  <span className="material-symbols-outlined text-primary">event</span>
                  <span className="font-body-sm text-[14px]">{intl.formatMessage({ id: 'settings.billing.tokenLimit' }, { limit: (plan?.token_limit?.toLocaleString() ?? '1,000,000') })}</span>
                </div>
                <div className="flex items-center gap-3 text-on-surface-variant">
                  <span className="material-symbols-outlined text-primary">credit_card</span>
                  <span className="font-body-sm text-[14px]">{intl.formatMessage({ id: 'settings.billing.provider' }, { provider: (status?.provider ?? 'N/A') })}</span>
                </div>
              </div>
            </div>
            <div className="flex gap-3 mt-auto">
              <Button
                disabled
                title={t('settings.billing.demoActionsDisabled')}
                className="flex-1 py-3 px-4 bg-primary text-on-primary rounded-xl font-bold text-center opacity-50 cursor-not-allowed"
              >{t('settings.billing.changePlan')}</Button>
              <Button
                disabled
                title={t('settings.billing.demoActionsDisabled')}
                className="px-4 py-3 border border-outline-variant text-on-surface-variant rounded-xl font-bold opacity-50 cursor-not-allowed"
              >{t('settings.billing.cancel')}</Button>
            </div>
          </section>

          {/* Section 2: Usage Quota Overview */}
          <section className="md:col-span-7 bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/30 rounded-2xl p-lg shadow-sm">
            <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest mb-lg">{t('settings.billing.usageOverview')}</h3>
            <div className="grid grid-cols-1 gap-lg md:grid-cols-2">
              {/* Token Usage Ring */}
              <div className="flex flex-col items-center text-center">
                <div className="relative w-28 h-28 mb-4 flex items-center justify-center">
                  <svg className="w-full h-full transform -rotate-90">
                    <circle className="text-surface-container-highest" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor" strokeWidth="8" />
                    <circle className="text-primary transition-all duration-1000 ease-out" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor"
                      strokeDasharray="301.6"
                      strokeDashoffset={totalTokens > 0 ? 301.6 - Math.min(301.6, (totalTokens / 1000000) * 301.6) : 301.6}
                      strokeWidth="8" />
                  </svg>
                  <div className="absolute flex flex-col items-center">
                    <span className="font-headline-md text-[24px] font-bold">
                      {totalTokens >= 1000000 ? `${(totalTokens / 1000000).toFixed(1)}M` : totalTokens >= 1000 ? `${(totalTokens / 1000).toFixed(0)}K` : '0'}
                    </span>
                  </div>
                </div>
                <p className="font-label-md text-[14px] font-bold mb-1">{t('settings.billing.tokenUsage')}</p>
                <p className="font-label-sm text-[12px] text-on-surface-variant">
                  {totalTokens >= 1000000 ? `${(totalTokens / 1000000).toFixed(1)}M` : totalTokens >= 1000 ? `${(totalTokens / 1000).toFixed(0)}K` : '0'}
                </p>
              </div>

              {/* Cache Hit Rate Ring */}
              <div className="flex flex-col items-center text-center">
                <div className="relative w-28 h-28 mb-4 flex items-center justify-center">
                  <svg className="w-full h-full transform -rotate-90">
                    <circle className="text-surface-container-highest" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor" strokeWidth="8" />
                    <circle className="text-secondary transition-all duration-1000 ease-out" cx="56" cy="56" fill="transparent" r="48" stroke="currentColor"
                      strokeDasharray="301.6"
                      strokeDashoffset={301.6 - cacheHitRate * 301.6}
                      strokeWidth="8" />
                  </svg>
                  <div className="absolute flex flex-col items-center">
                    <span className="font-headline-md text-[24px] font-bold">{Math.round(cacheHitRate * 100)}%</span>
                  </div>
                </div>
                <p className="font-label-md text-[14px] font-bold mb-1">{t('settings.billing.cacheHitRate')}</p>
                <p className="font-label-sm text-[12px] text-on-surface-variant">{t('settings.billing.avgCacheHit')}</p>
              </div>
            </div>
          </section>

          {/* Section 3: Cost Analysis Chart */}
          <section className="md:col-span-12 bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/30 rounded-2xl p-lg shadow-sm">
            <div className="flex justify-between items-end mb-xl">
              <div>
                <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest mb-2">{t('settings.billing.costAnalysis')}</h3>
                <p className="font-headline-md text-[24px] font-bold">{t('settings.billing.dailySpending')} <span className="text-on-surface-variant font-normal text-[14px] ml-1">{t('settings.billing.last30Days')}</span></p>
              </div>
              <div className="flex gap-2">
                <span className="flex items-center gap-2 font-label-md text-[14px] text-on-surface-variant bg-surface-container px-3 py-1 rounded-lg">
                  <span className="w-2 h-2 rounded-full bg-primary"></span>{t('settings.billing.tokens')}
                </span>
                <span className="flex items-center gap-2 font-label-md text-[14px] text-on-surface-variant bg-surface-container px-3 py-1 rounded-lg">
                  <span className="w-2 h-2 rounded-full bg-secondary"></span> {t('settings.billing.cacheHit')}
                </span>
              </div>
            </div>

            {costHistory.length > 0 ? (
              <>
                <div className="h-48 flex items-end justify-between gap-2 px-2">
                  {costHistory.slice(-10).map((r, i) => (
                    <div key={i} className="w-full flex flex-col justify-end group relative cursor-pointer hover:brightness-110 transition-all" style={{ height: `${Math.max(8, (r.cost_usd / maxCost) * 100)}%` }}>
                      <div className="w-full bg-primary flex-1 rounded-t-sm transition-all duration-1000 ease-out"></div>
                      <div className="w-full bg-secondary h-[30%] transition-all duration-1000 ease-out"></div>
                      <div className="absolute -top-8 left-1/2 -translate-x-1/2 bg-surface-container-lowest border border-outline-variant/30 rounded px-2 py-1 text-label-sm font-bold opacity-0 group-hover:opacity-100 transition-opacity whitespace-nowrap pointer-events-none">
                        ${r.cost_usd.toFixed(4)}
                      </div>
                    </div>
                  ))}
                </div>
                <div className="flex justify-between mt-4 px-2 text-on-surface-variant font-label-sm text-[12px]">
                  {costHistory.length >= 10 && <span>{costHistory[costHistory.length - 10]?.date}</span>}
                  <span>{costHistory[costHistory.length - 5]?.date ?? ''}</span>
                  <span>{t('settings.billing.today')}</span>
                </div>
              </>
            ) : (
              <div className="h-48 flex items-center justify-center">
                <p className="text-body-sm text-on-surface-variant">{t('settings.billing.noCostData')}</p>
              </div>
            )}
          </section>

          {/* Section 4: Billing History */}
          <section className="md:col-span-12 bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/30 rounded-2xl p-lg overflow-hidden shadow-sm">
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">{t('settings.billing.billingHistory')}</h3>
            </div>
            <div className="overflow-x-auto">
              <table className="w-full text-left">
                <thead>
                  <tr className="border-b border-outline-variant/30 font-label-sm text-[12px] text-on-surface-variant uppercase tracking-wider">
                    <th className="pb-4 font-medium px-2">{t('settings.billing.colDate')}</th>
                    <th className="pb-4 font-medium px-2">{t('settings.billing.colDescription')}</th>
                    <th className="pb-4 font-medium px-2 text-right">{t('settings.billing.colAmount')}</th>
                    <th className="pb-4 font-medium px-2 text-center">{t('settings.billing.colStatus')}</th>
                  </tr>
                </thead>
                <tbody className="font-body-sm text-[14px]">
                  {billingHistory.length > 0 ? billingHistory.map(bh => (
                    <tr key={bh.id} className="border-b border-outline-variant/10 group hover:bg-surface-container-low transition-colors">
                      <td className="py-4 px-2 text-on-surface-variant">{bh.date}</td>
                      <td className="py-4 px-2 font-medium">{bh.description}</td>
                      <td className="py-4 px-2 text-right">${bh.amount.toFixed(2)}</td>
                      <td className="py-4 px-2 text-center">
                        <span className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-[11px] font-bold uppercase tracking-wider ${
                          bh.status === 'paid' ? 'bg-tertiary/10 text-tertiary' : bh.status === 'pending' ? 'bg-surface-container-high text-on-surface-variant' : 'bg-error/10 text-error'
                        }`}>
                          <span className={`w-1.5 h-1.5 rounded-full ${
                            bh.status === 'paid' ? 'bg-tertiary' : bh.status === 'pending' ? 'bg-on-surface-variant/40' : 'bg-error'
                          }`}></span>
                          {bh.status}
                        </span>
                      </td>
                    </tr>
                  )) : (
                    <>
                      <tr className="border-b border-outline-variant/10 group hover:bg-surface-container-low transition-colors">
                        <td className="py-4 px-2 text-on-surface-variant">—</td>
                        <td className="py-4 px-2 font-medium">{t('settings.billing.noRecords')}</td>
                        <td className="py-4 px-2 text-right">—</td>
                        <td className="py-4 px-2 text-center">
                          <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-surface-container text-on-surface-variant text-[11px] font-bold uppercase tracking-wider">
                            <span className="w-1.5 h-1.5 rounded-full bg-on-surface-variant/40"></span> N/A
                          </span>
                        </td>
                      </tr>
                    </>
                  )}
                </tbody>
              </table>
            </div>
          </section>
        </div>
        )}
      </div>

      {/* Footer Help Section */}
      <footer className="mt-xl flex flex-col md:flex-row justify-between items-center px-lg py-md bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/30 rounded-2xl shadow-sm gap-md">
        <div className="flex items-center gap-4 text-center md:text-left">
          <span className="material-symbols-outlined text-primary hidden md:block">info</span>
          <p className="font-body-sm text-[14px] text-on-surface-variant">{intl.formatMessage({ id: 'settings.billing.enterpriseCta' }, { team: t('settings.billing.enterpriseTeam') })}</p>
        </div>
        <div className="flex items-center justify-center gap-6">
          <button className="font-label-sm text-[12px] text-on-surface-variant hover:text-on-surface transition-colors cursor-pointer" onClick={() => setShowLegal(true)}>{t('settings.billing.legalTerms')}</button>
          <button className="font-label-sm text-[12px] text-on-surface-variant hover:text-on-surface transition-colors cursor-pointer" onClick={() => setShowLegal(true)}>{t('settings.billing.privacyPolicy')}</button>
        </div>
      </footer>

      {/* Cancel Subscription Modal */}
      {showCancelConfirm && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowCancelConfirm(false)} onKeyDown={e => { if (e.key === 'Escape') setShowCancelConfirm(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">warning</span>
              <h3 className="font-headline-md text-on-surface">{t('settings.billing.cancelTitle')}</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">{t('settings.billing.cancelConfirm')}</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setShowCancelConfirm(false)}>{t('settings.billing.keepPlan')}</Button>
              <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={handleCancelSubscription} disabled={cancelling}>{cancelling ? t('settings.billing.cancelling') : t('settings.billing.cancelPlan')}</Button>
            </div>
          </div>
        </div>
      )}

      {/* Change Plan Modal */}
      {showChangePlan && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 flex items-center justify-center z-50" onClick={() => setShowChangePlan(false)} onKeyDown={e => { if (e.key === 'Escape') setShowChangePlan(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl max-w-md w-full mx-lg shadow-2xl" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-headline-md text-on-surface">{t('settings.billing.changePlanTitle')}</h3>
              <Button variant="ghost" className="cursor-pointer" onClick={() => setShowChangePlan(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <div className="space-y-md">
              {(['Free', 'Pro', 'Enterprise'] as const).map(p => (
                <button key={p} disabled={changingPlan !== null} className={`w-full p-md rounded-xl border text-left cursor-pointer transition-all disabled:opacity-50 ${plan?.name?.toLowerCase() === p.toLowerCase() ? 'border-2 border-primary bg-primary/5' : 'border-outline-variant/30 hover:border-primary/50'}`} onClick={() => handleChangePlan(p)}>
                  <div className="font-label-md font-bold text-on-surface">{p}</div>
                  <div className="font-label-sm text-on-surface-variant">{p === 'Free' ? t('settings.billing.planFreeDesc') : p === 'Pro' ? t('settings.billing.planProDesc') : t('settings.billing.planEnterpriseDesc')}</div>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Legal Modal */}
      {showLegal && (
        <div role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 flex items-center justify-center z-50" onClick={() => setShowLegal(false)} onKeyDown={e => { if (e.key === 'Escape') setShowLegal(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl max-w-lg w-full mx-lg shadow-2xl max-h-[80vh] overflow-y-auto" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-headline-md text-on-surface">{t('settings.billing.legalPrivacy')}</h3>
              <Button variant="ghost" className="cursor-pointer" onClick={() => setShowLegal(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <div className="text-body-sm text-on-surface-variant space-y-md">
              <p>{t('settings.billing.legalBody1')}</p>
              <p>{t('settings.billing.legalBody2')}</p>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
