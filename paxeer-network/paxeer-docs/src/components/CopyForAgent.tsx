'use client'

import { useState } from 'react'

export function CopyForAgent({ pageTitle, children }: { pageTitle: string; children: React.ReactNode }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = async () => {
    const article = document.querySelector('article.article')
    if (!article) return

    const clone = article.cloneNode(true) as HTMLElement
    clone.querySelectorAll('nav, aside, .copy-for-agent-btn').forEach((el) => el.remove())

    const markdown = `# ${pageTitle}\n\n${clone.textContent?.trim() || ''}`

    try {
      await navigator.clipboard.writeText(markdown)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      const field = document.createElement('textarea')
      field.value = markdown
      field.style.position = 'fixed'
      field.style.opacity = '0'
      document.body.appendChild(field)
      field.select()
      document.execCommand('copy')
      field.remove()
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }

  return (
    <button
      onClick={handleCopy}
      className="copy-for-agent-btn inline-flex items-center gap-2 px-4 py-2 bg-secondary-container text-on-secondary-container rounded-full text-sm font-medium hover:bg-secondary-container/90 transition-all duration-150"
    >
      {copied ? (
        <>
          <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none">
            <path d="m5 13 4 4L19 7" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          <span>Copied</span>
        </>
      ) : (
        <>
          <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none">
            <rect x="9" y="9" width="13" height="13" rx="2" stroke="currentColor" strokeWidth="1.7" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" stroke="currentColor" strokeWidth="1.7" />
          </svg>
          <span>Copy for agent</span>
        </>
      )}
    </button>
  )
}
