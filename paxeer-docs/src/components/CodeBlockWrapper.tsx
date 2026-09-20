'use client'

import { useEffect } from 'react'

export function CodeBlockWrapper() {
  useEffect(() => {
    const addCopyButtons = () => {
      const codeBlocks = document.querySelectorAll('pre')
      
      codeBlocks.forEach((pre) => {
        if (pre.querySelector('.copy-button')) return

        const button = document.createElement('button')
        button.className = 'copy-button absolute top-2 right-2 p-2 rounded-md bg-surface-container hover:bg-surface-high transition-colors text-on-surface-variant hover:text-on-surface opacity-0 group-hover:opacity-100'
        button.title = 'Copy code'
        
        const iconCopy = `<svg class="w-4 h-4" viewBox="0 0 24 24" fill="none"><rect x="9" y="9" width="13" height="13" rx="2" stroke="currentColor" stroke-width="1.7"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" stroke="currentColor" stroke-width="1.7"/></svg>`
        const iconCheck = `<svg class="w-4 h-4" viewBox="0 0 24 24" fill="none"><path d="m5 13 4 4L19 7" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`
        
        button.innerHTML = iconCopy
        
        button.addEventListener('click', async () => {
          const code = pre.querySelector('code')
          const text = code?.textContent || ''
          
          try {
            await navigator.clipboard.writeText(text)
            button.innerHTML = iconCheck
            setTimeout(() => {
              button.innerHTML = iconCopy
            }, 2000)
          } catch (err) {
            const field = document.createElement('textarea')
            field.value = text
            field.style.position = 'fixed'
            field.style.opacity = '0'
            document.body.appendChild(field)
            field.select()
            document.execCommand('copy')
            field.remove()
            button.innerHTML = iconCheck
            setTimeout(() => {
              button.innerHTML = iconCopy
            }, 2000)
          }
        })
        
        pre.style.position = 'relative'
        pre.classList.add('group')
        pre.appendChild(button)
      })
    }

    addCopyButtons()
    
    const observer = new MutationObserver(addCopyButtons)
    observer.observe(document.body, { childList: true, subtree: true })
    
    return () => observer.disconnect()
  }, [])

  return null
}
