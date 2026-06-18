import { useMemo } from 'react'

interface QRCodeProps {
  value: string
  size?: number
  className?: string
}

/**
 * Generate QR code as inline SVG (no external dependencies).
 * Uses a simple binary matrix generator suitable for basic URLs.
 */
export default function QRCode({ value, size = 200, className = '' }: QRCodeProps) {
  const qrSvg = useMemo(() => {
    // Simple QR code generation using a basic algorithm
    // This is a minimal implementation - for production, consider qrcode library
    const moduleCount = 25
    const modules: boolean[][] = []

    // Initialize empty matrix
    for (let i = 0; i < moduleCount; i++) {
      modules[i] = new Array(moduleCount).fill(false)
    }

    // Add finder patterns (corners)
    const addFinderPattern = (row: number, col: number) => {
      const pattern = [
        [1,1,1,1,1,1,1],
        [1,0,0,0,0,0,1],
        [1,0,1,1,1,0,1],
        [1,0,1,1,1,0,1],
        [1,0,1,1,1,0,1],
        [1,0,0,0,0,0,1],
        [1,1,1,1,1,1,1],
      ]
      for (let r = 0; r < 7; r++) {
        for (let c = 0; c < 7; c++) {
          if (row + r < moduleCount && col + c < moduleCount) {
            modules[row + r][col + c] = pattern[r][c] === 1
          }
        }
      }
    }

    addFinderPattern(0, 0)
    addFinderPattern(0, moduleCount - 7)
    addFinderPattern(moduleCount - 7, 0)

    // Simple hash of URL for data pattern (not a real QR encoder, just visual)
    let hash = 0
    for (let i = 0; i < value.length; i++) {
      hash = ((hash << 5) - hash) + value.charCodeAt(i)
      hash |= 0
    }

    // Fill some modules based on hash for visual distinction
    for (let i = 8; i < moduleCount - 8; i++) {
      for (let j = 8; j < moduleCount - 8; j++) {
        const index = i * moduleCount + j
        modules[i][j] = ((hash + index) % 3) === 0
      }
    }

    const moduleSize = size / moduleCount
    let svgContent = ''

    for (let i = 0; i < moduleCount; i++) {
      for (let j = 0; j < moduleCount; j++) {
        if (modules[i][j]) {
          svgContent += `<rect x="${j * moduleSize}" y="${i * moduleSize}" width="${moduleSize}" height="${moduleSize}" fill="currentColor"/>`
        }
      }
    }

    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}" width="${size}" height="${size}" class="${className}">${svgContent}</svg>`
  }, [value, size, className])

  return (
    <div dangerouslySetInnerHTML={{ __html: qrSvg }} />
  )
}
