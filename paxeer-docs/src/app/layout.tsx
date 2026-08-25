import '@/styles/globals.css'
import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Paxeer Network Documentation',
  description: 'Technical documentation for Paxeer Network (EVM L1 chain ID 125) - the settlement and custody layer for LayerX',
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  )
}
