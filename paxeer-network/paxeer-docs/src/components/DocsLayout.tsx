'use client'

import { ReactNode } from 'react'
import { Sidebar } from './Sidebar'
import { CopyForAgent } from './CopyForAgent'

export function DocsLayout({ children, pageTitle }: { children: ReactNode; pageTitle?: string }) {
  return (
    <div className="flex min-h-screen bg-surface">
      <Sidebar />
      <main className="flex-1 ml-[280px] p-12 max-w-[1200px]">
        {pageTitle && (
          <div className="mb-6 flex items-center justify-between pb-6 border-b border-outline-variant">
            <h1 className="text-5xl font-light tracking-[-0.02em]">{pageTitle}</h1>
            <CopyForAgent pageTitle={pageTitle}>{children}</CopyForAgent>
          </div>
        )}
        <article className="article max-w-[820px]">
          {children}
        </article>
      </main>
    </div>
  )
}
