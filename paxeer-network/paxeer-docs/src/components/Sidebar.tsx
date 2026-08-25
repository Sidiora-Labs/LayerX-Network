'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { navStructure } from './nav'
import { Search } from './Search'

export function Sidebar() {
  const pathname = usePathname()

  return (
    <aside className="fixed top-0 left-0 h-screen w-[280px] bg-surface border-r border-outline-variant overflow-y-auto">
      <div className="px-6 py-8 border-b border-outline-variant">
        <Link href="/" className="block">
          <div className="text-lg font-medium text-on-surface">Paxeer Network</div>
          <div className="text-xs text-on-surface-variant font-mono uppercase tracking-wider mt-1">
            Chain ID 125 Docs
          </div>
        </Link>
      </div>
      
      <div className="px-4 py-4">
        <Search />
      </div>

      <nav className="px-4 pb-8">
        {navStructure.map((section) => (
          <div key={section.section} className="mb-6">
            <div className="text-xs font-medium text-on-surface-variant uppercase tracking-wider px-2 mb-2">
              {section.section}
            </div>
            {section.items.map((item) => {
              const isActive = pathname === item.href
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={`block px-2 py-2 rounded-md text-sm transition-all duration-150 ${
                    isActive
                      ? 'bg-surface-high text-ink-text font-medium'
                      : 'text-on-surface-variant hover:bg-surface-high hover:text-on-surface'
                  }`}
                >
                  {item.title}
                </Link>
              )
            })}
          </div>
        ))}
      </nav>
    </aside>
  )
}
