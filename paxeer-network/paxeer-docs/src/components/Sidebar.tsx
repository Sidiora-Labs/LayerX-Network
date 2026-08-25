'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { navStructure } from './nav'
import { Search } from './Search'

export function Sidebar({ open, onClose }: { open: boolean; onClose: () => void }) {
  const pathname = usePathname()

  return (
    <>
      {open && (
        <button
          type="button"
          aria-label="Close documentation navigation"
          className="fixed inset-0 z-30 bg-black/60 md:hidden"
          onClick={onClose}
        />
      )}
      <aside
        id="docs-navigation"
        className={`fixed inset-y-0 left-0 z-40 w-[280px] overflow-y-auto border-r border-outline-variant bg-surface transition-transform duration-300 md:translate-x-0 ${
          open ? 'translate-x-0' : '-translate-x-full'
        }`}
      >
        <div className="px-6 py-8 border-b border-outline-variant">
          <Link href="/" className="block" onClick={onClose}>
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
                    onClick={onClose}
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
    </>
  )
}
