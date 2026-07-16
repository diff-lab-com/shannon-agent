import { useIntl } from 'react-intl'
import { Badge } from '@/components/ui/badge'

export function AgentAuthoredBadge() {
  const intl = useIntl()
  const label = intl.formatMessage({ id: 'skills.badge.agentAuthored' })
  return (
    <Badge variant="tertiary" size="sm" title={label}>
      <span className="material-symbols-outlined icon-xs mr-[2px]">auto_fix</span>
      {label}
    </Badge>
  )
}
