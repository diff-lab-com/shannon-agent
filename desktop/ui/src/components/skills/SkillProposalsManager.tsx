// Global skill-proposals surface (IA X1).
//
// Reduced to the lightweight discovery toast: it listens for pending
// candidates everywhere and nudges the user to /extensions/pending. The
// full review UI (candidates queue + proposal drafts) lives on the
// Extensions → Pending page (评审裁决 #2: 技能提案主审查面 = 扩展-待处理).

import SkillProposalsToast from './SkillProposalsToast'

export default function SkillProposalsManager() {
  return <SkillProposalsToast />
}
