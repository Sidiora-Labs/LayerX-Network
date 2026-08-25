import { ReactNode } from 'react'
import { Sidebar } from './Sidebar'

export function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <div className="layout">
      <Sidebar />
      <main className="content">
        <article className="article">
          {children}
        </article>
      </main>
    </div>
  )
}
