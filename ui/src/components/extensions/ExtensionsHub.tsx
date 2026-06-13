import { useState, useEffect } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import * as api from '@/lib/tauri-api'
import type { SkillInfo } from '@/types'

export default function ExtensionsHub() {
  const [skills, setSkills] = useState<SkillInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [filterMode, setFilterMode] = useState<'trending' | 'recent'>('trending')
  const [selectedSkill, setSelectedSkill] = useState<SkillInfo | null>(null)
  const [searchQuery, setSearchQuery] = useState('')

  useEffect(() => {
    api.listSkills()
      .then(setSkills)
      .catch(e => { console.warn('Failed to load skills:', e); toast.error('Failed to load skills') })
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    if (!selectedSkill) return
    const handleKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setSelectedSkill(null) }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [selectedSkill])

  const filteredSkills = skills.filter(s => !searchQuery ||
    s.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    s.description?.toLowerCase().includes(searchQuery.toLowerCase())
  )

  const categories = [...new Set(filteredSkills.map(s => s.category ?? 'Uncategorized'))]
  const sortedCategories = filterMode === 'recent' ? [...categories].reverse() : categories

  const sortedSkills = (cat: string) => {
    const catSkills = filteredSkills.filter(s => (s.category ?? 'Uncategorized') === cat)
    return filterMode === 'recent' ? [...catSkills].reverse() : catSkills
  }

  const iconForCategory = (cat: string) => {
    switch (cat.toLowerCase()) {
      case 'productivity': return 'bolt'
      case 'design': return 'palette'
      case 'data': case 'analysis': return 'analytics'
      case 'code': return 'code'
      default: return 'extension'
    }
  }

  const colorForCategory = (cat: string) => {
    switch (cat.toLowerCase()) {
      case 'productivity': return 'bg-primary/10 text-primary'
      case 'design': return 'bg-secondary/10 text-secondary'
      case 'data': case 'analysis': return 'bg-tertiary/10 text-tertiary'
      case 'code': return 'bg-tertiary/10 text-tertiary'
      default: return 'bg-surface-container-high text-on-surface-variant'
    }
  }

  return (
    <div className="max-w-[1200px] mx-auto px-lg pt-lg pb-xl">
      <section className="mb-xl mt-4">
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-headline-md">Available Skills</h3>
          <div className="flex items-center gap-sm">
            <div className="relative">
              <span className="material-symbols-outlined absolute left-sm top-1/2 -translate-y-1/2 text-on-surface-variant text-[18px]">search</span>
              <input
                type="text"
                value={searchQuery}
                onChange={e => setSearchQuery(e.target.value)}
                placeholder="Search skills..."
                className="pl-[36px] pr-md py-xs rounded-lg bg-surface-container-low border border-outline-variant/30 text-body-sm text-on-surface placeholder:text-on-surface-variant/50 outline-none focus:ring-2 focus:ring-primary/20 w-[200px]"
              />
            </div>
            <div className="flex bg-surface-container-low rounded-lg p-xs gap-xs">
              <button onClick={() => setFilterMode('trending')} className={`px-sm py-xs rounded-md text-label-sm font-bold cursor-pointer transition-colors focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none ${filterMode === 'trending' ? 'bg-surface-container-lowest text-primary shadow-sm' : 'text-on-surface-variant hover:bg-surface-container-high'}`}>Trending</button>
              <button onClick={() => setFilterMode('recent')} className={`px-sm py-xs rounded-md text-label-sm font-bold cursor-pointer transition-colors focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none ${filterMode === 'recent' ? 'bg-surface-container-lowest text-primary shadow-sm' : 'text-on-surface-variant hover:bg-surface-container-high'}`}>Recent</button>
            </div>
            <Button variant="ghost" className="px-md py-sm rounded-full bg-surface-container-high font-label-md text-label-md text-on-surface cursor-pointer">
              {filteredSkills.length} Skills
            </Button>
          </div>
        </div>

        {loading ? (
          <div className="flex items-center justify-center py-xl">
            <span className="material-symbols-outlined animate-spin text-[32px] text-primary">progress_activity</span>
          </div>
        ) : filteredSkills.length === 0 ? (
          <EmptyState
            icon="extension_off"
            title={searchQuery ? 'No skills match your search.' : 'No skills available.'}
            description={searchQuery ? 'Try a different search term.' : 'Skills can be added via MCP servers or plugin configuration.'}
          />
        ) : (
          sortedCategories.map(cat => (
            <div key={cat} className="mb-lg">
              <h4 className="font-label-md text-label-md text-outline uppercase tracking-widest mb-md">{cat}</h4>
              <div className="flex flex-wrap gap-md">
                {sortedSkills(cat).map(skill => (
                  <div key={skill.name} role="button" tabIndex={0} className="group cursor-pointer bg-surface-container-lowest border border-outline-variant/50 rounded-xl p-md flex items-center gap-md hover:border-primary transition-all shadow-sm focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={() => setSelectedSkill(selectedSkill?.name === skill.name ? null : skill)} onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setSelectedSkill(selectedSkill?.name === skill.name ? null : skill) } }}>
                    <div className={`w-10 h-10 rounded-lg flex items-center justify-center ${colorForCategory(cat)}`}>
                      <span className="material-symbols-outlined">{iconForCategory(cat)}</span>
                    </div>
                    <div className="flex-1 min-w-0">
                      <p className="font-label-md text-label-md font-bold">{skill.name}</p>
                      <p className="text-label-sm font-label-sm text-on-surface-variant truncate">{skill.description || `Trigger: ${skill.trigger}`}</p>
                    </div>
                    <span className="px-sm py-xs bg-surface-container-low rounded-full text-label-sm font-label-sm text-on-surface-variant">{skill.source}</span>
                    <span className="material-symbols-outlined text-[16px] text-on-surface-variant">{selectedSkill?.name === skill.name ? 'expand_less' : 'expand_more'}</span>
                  </div>
                ))}
                {sortedSkills(cat).map(skill => selectedSkill?.name === skill.name && (
                  <div key={`${skill.name}-detail`} className="bg-surface-container-low border border-primary/30 rounded-xl p-lg space-y-sm">
                    <h5 className="font-label-md text-on-surface font-bold">{skill.name}</h5>
                    <p className="text-body-sm text-on-surface-variant">{skill.description || 'No description available.'}</p>
                    <div className="flex items-center gap-md text-label-sm text-on-surface-variant">
                      <span className="flex items-center gap-xs"><span className="material-symbols-outlined text-[14px]">terminal</span>Trigger: {skill.trigger}</span>
                      <span className="flex items-center gap-xs"><span className="material-symbols-outlined text-[14px]">source</span>{skill.source}</span>
                    </div>
                    <button className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md hover:opacity-90 transition-all" onClick={() => setSelectedSkill(null)}>Close</button>
                  </div>
                ))}
              </div>
            </div>
          ))
        )}
      </section>
    </div>
  )
}
