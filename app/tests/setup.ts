import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

afterEach(() => cleanup())

Object.defineProperty(window.navigator, 'platform', {
  configurable: true,
  value: 'MacIntel',
})

Object.defineProperty(window, 'matchMedia', {
  configurable: true,
  value: (query: string): MediaQueryList => ({
    addEventListener: () => undefined,
    addListener: () => undefined,
    dispatchEvent: () => false,
    matches: false,
    media: query,
    onchange: null,
    removeEventListener: () => undefined,
    removeListener: () => undefined,
  }),
})

globalThis.ResizeObserver = class ResizeObserver {
  disconnect() {}
  observe() {}
  unobserve() {}
}

if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList
}

if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = () => new DOMRect()
}

HTMLElement.prototype.scrollIntoView = () => undefined

document.elementFromPoint = () => null

globalThis.requestAnimationFrame = (callback: FrameRequestCallback) => {
  return window.setTimeout(() => callback(performance.now()), 0)
}

globalThis.cancelAnimationFrame = (handle: number) => window.clearTimeout(handle)
