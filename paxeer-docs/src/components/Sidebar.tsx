'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { navStructure } from './nav'

export function Sidebar() {
  const pathname = usePathname()

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <Link href="/" className="sidebar-title">
          Paxeer Network
        </Link>
        <div className="sidebar-subtitle">Chain ID 125 Technical Docs</div>
      </div>
      <nav className="nav">
        {navStructure.map((section) => (
          <div key={section.section} className="nav-section">
            <div className="nav-section-title">{section.section}</div>
            {section.items.map((item) => (
              <Link
                key={item.href}
                href={item.href}
                className={`nav-item ${pathname === item.href ? 'active' : ''}`}
              >
                {item.title}
              </Link>
            ))}
          </div>
        ))}
      </nav>
    </aside>
  )
}
