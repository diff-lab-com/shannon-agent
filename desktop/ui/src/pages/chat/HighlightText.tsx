import { memo } from 'react'

// Tiny presentational memo used inside the session list to highlight the
// matched substring of the current search query. Memoized because the list
// re-renders on every keystroke and HighlightText otherwise does redundant
// toLowerCase work for each session row.
const HighlightText = memo(function HighlightText({ text, query }: { text: string; query: string }) {
  if (!query) return <>{text}</>
  const idx = text.toLowerCase().indexOf(query.toLowerCase())
  if (idx === -1) return <>{text}</>
  return (
    <>
      {text.slice(0, idx)}
      <mark className="bg-primary/20 text-inherit rounded-sm px-[1px]">{text.slice(idx, idx + query.length)}</mark>
      {text.slice(idx + query.length)}
    </>
  )
})

export default HighlightText