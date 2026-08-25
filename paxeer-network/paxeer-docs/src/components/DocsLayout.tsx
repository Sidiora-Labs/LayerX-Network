'use client'

import { ReactNode, useEffect, useState } from 'react'
import { usePathname } from 'next/navigation'
import { Sidebar } from './Sidebar'
import { CopyForAgent } from './CopyForAgent'
import { pageTitleForPath } from './nav'

export function DocsLayout({ children }: { children: ReactNode }) {
  const pathname = usePathname()
  const [navigationOpen, setNavigationOpen] = useState(false)
  const pageTitle = pageTitleForPath(pathname)

  useEffect(() => {
    if (!navigationOpen) return
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setNavigationOpen(false)
    }
    document.addEventListener('keydown', closeOnEscape)
    return () => document.removeEventListener('keydown', closeOnEscape)
  }, [navigationOpen])

  return (
    <div className="flex min-h-screen bg-surface">
      <Sidebar open={navigationOpen} onClose={() => setNavigationOpen(false)} />
      <main className="w-full min-w-0 flex-1 px-5 py-6 sm:px-8 md:ml-[280px] md:p-12">
        <div className="mb-6 flex items-center justify-between gap-3 border-b border-outline-variant pb-5">
          <button
            type="button"
            aria-controls="docs-navigation"
            aria-expanded={navigationOpen}
            onClick={() => setNavigationOpen(true)}
            className="inline-flex min-h-11 items-center gap-2 rounded-full bg-secondary-container px-4 text-sm font-medium text-on-secondary-container md:hidden"
          >
            <span aria-hidden="true">☰</span>
            Navigation
          </button>
          <div className="ml-auto">
            <CopyForAgent pageTitle={pageTitle}>{children}</CopyForAgent>
          </div>
        </div>
        <article className="article max-w-[820px]">
          {children}
        </article>
      </main>
    </div>
  )
}
