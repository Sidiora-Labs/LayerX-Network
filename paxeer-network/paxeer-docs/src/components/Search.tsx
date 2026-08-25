'use client'

import { useState, useEffect, useId, useRef } from 'react'
import { useRouter } from 'next/navigation'
import { navStructure } from './nav'

interface SearchResult {
  title: string
  href: string
  section: string
}

export function Search() {
  const [isOpen, setIsOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<SearchResult[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const router = useRouter()
  const inputRef = useRef<HTMLInputElement>(null)
  const dialogTitleId = useId()
  const resultsId = useId()

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === 'k' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault()
        setIsOpen((open) => !open)
      }
      if (e.key === 'Escape') {
        setIsOpen(false)
      }
    }

    document.addEventListener('keydown', down)
    return () => document.removeEventListener('keydown', down)
  }, [])

  useEffect(() => {
    if (isOpen && inputRef.current) {
      inputRef.current.focus()
    }
  }, [isOpen])

  useEffect(() => {
    if (!query.trim()) {
      setResults([])
      return
    }

    const allPages: SearchResult[] = navStructure.flatMap((section) =>
      section.items.map((item) => ({
        title: item.title,
        href: item.href,
        section: section.section,
      }))
    )

    const filtered = allPages.filter(
      (page) =>
        page.title.toLowerCase().includes(query.toLowerCase()) ||
        page.section.toLowerCase().includes(query.toLowerCase())
    )

    setResults(filtered)
    setSelectedIndex(0)
  }, [query])

  const handleSelect = (href: string) => {
    router.push(href)
    setIsOpen(false)
    setQuery('')
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (results.length === 0) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex((i) => (i + 1) % results.length)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex((i) => (i - 1 + results.length) % results.length)
    } else if (e.key === 'Enter' && results[selectedIndex]) {
      handleSelect(results[selectedIndex].href)
    }
  }

  if (!isOpen) {
    return (
      <button
        type="button"
        aria-haspopup="dialog"
        onClick={() => setIsOpen(true)}
        className="flex items-center gap-2 px-3 py-1.5 bg-surface-high rounded-md border border-outline-variant hover:border-outline transition-all text-sm text-on-surface-variant"
      >
        <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none">
          <circle cx="11" cy="11" r="7" stroke="currentColor" strokeWidth="1.7" />
          <path d="m16 16 4.5 4.5" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
        </svg>
        <span>Search</span>
        <kbd className="ml-1 px-1.5 py-0.5 bg-surface-highest rounded text-xs font-mono">⌘K</kbd>
      </button>
    )
  }

  return (
    <>
      <button
        type="button"
        aria-label="Close documentation search"
        className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm"
        onClick={() => setIsOpen(false)}
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={dialogTitleId}
        className="fixed inset-x-0 top-20 z-50 mx-auto max-w-2xl px-4"
      >
        <div className="bg-surface-high rounded-xl border border-outline shadow-3 overflow-hidden">
          <h2 id={dialogTitleId} className="sr-only">Search Paxeer documentation</h2>
          <div className="flex items-center gap-3 px-4 py-3 border-b border-outline-variant">
            <svg className="w-5 h-5 text-on-surface-variant" viewBox="0 0 24 24" fill="none">
              <circle cx="11" cy="11" r="7" stroke="currentColor" strokeWidth="1.7" />
              <path d="m16 16 4.5 4.5" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
            </svg>
            <input
              ref={inputRef}
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              role="combobox"
              aria-autocomplete="list"
              aria-controls={resultsId}
              aria-expanded={results.length > 0}
              aria-activedescendant={results[selectedIndex] ? `${resultsId}-${selectedIndex}` : undefined}
              placeholder="Search documentation..."
              className="flex-1 bg-transparent border-none outline-none text-on-surface placeholder:text-on-surface-variant"
            />
            {query && (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => setQuery('')}
                className="text-on-surface-variant hover:text-on-surface"
              >
                <svg className="w-4 h-4" viewBox="0 0 24 24" fill="none">
                  <path d="M18 6 6 18M6 6l12 12" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
                </svg>
              </button>
            )}
          </div>
          {results.length > 0 ? (
            <div id={resultsId} role="listbox" className="max-h-96 overflow-y-auto">
              {results.map((result, index) => (
                <button
                  type="button"
                  id={`${resultsId}-${index}`}
                  role="option"
                  aria-selected={index === selectedIndex}
                  key={result.href}
                  onClick={() => handleSelect(result.href)}
                  className={`w-full text-left px-4 py-3 border-b border-outline-variant last:border-b-0 transition-colors ${
                    index === selectedIndex
                      ? 'bg-primary-container text-on-primary-container'
                      : 'hover:bg-surface-highest'
                  }`}
                >
                  <div className="font-medium">{result.title}</div>
                  <div className="text-xs text-on-surface-variant mt-0.5">{result.section}</div>
                </button>
              ))}
            </div>
          ) : query ? (
            <div className="px-4 py-8 text-center text-on-surface-variant">No results found</div>
          ) : null}
        </div>
      </div>
    </>
  )
}
