// Manages skill proposals toast and review panel.
//
// Wrapper component that maintains the review panel open state and
// passes it down to both toast (to open) and panel (to render).

import { useState } from 'react'
import SkillProposalsToast from './SkillProposalsToast'
import SkillProposalReviewPanel from './SkillProposalReviewPanel'

export default function SkillProposalsManager() {
  const [reviewOpen, setReviewOpen] = useState(false)

  const handleOpenReview = () => {
    setReviewOpen(true)
  }

  const handleCloseReview = () => {
    setReviewOpen(false)
  }

  return (
    <>
      <SkillProposalsToast onOpenReview={handleOpenReview} />
      <SkillProposalReviewPanel open={reviewOpen} onClose={handleCloseReview} />
    </>
  )
}
