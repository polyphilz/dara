import {
  Children,
  Component,
  useMemo,
  type ErrorInfo,
  type MouseEvent,
  type ReactNode,
} from 'react'
import ReactMarkdown, { type Components } from 'react-markdown'
import { native } from '../lib/native.ts'
import {
  localMediaUrl,
  parseImageReferenceToken,
} from '../media/image-reference.ts'
import { rehypePlugins, remarkPlugins } from './renderer-config.ts'
import { externalHttpUrl, markdownUrlTransform } from './url-policy.ts'

interface MarkdownRendererProps {
  openExternalUrl?: (url: string) => Promise<void> | void
  source: string
}

export function MarkdownRenderer({
  openExternalUrl = native.openExternalUrl,
  source,
}: MarkdownRendererProps) {
  const components = useMemo(
    () => rendererComponents(openExternalUrl),
    [openExternalUrl],
  )

  return (
    <MarkdownErrorBoundary key={source} source={source}>
      <div className="dara-markdown">
        <ReactMarkdown
          components={components}
          rehypePlugins={rehypePlugins}
          remarkPlugins={remarkPlugins}
          urlTransform={markdownUrlTransform}
        >
          {source}
        </ReactMarkdown>
      </div>
    </MarkdownErrorBoundary>
  )
}

function rendererComponents(
  openExternalUrl: (url: string) => Promise<void> | void,
): Components {
  return {
    a({ children, href }) {
      const url = externalHttpUrl(href)
      if (!url) {
        return <span className="dara-markdown-inert-link">{children}</span>
      }
      const openLink = (event: MouseEvent<HTMLAnchorElement>) => {
        event.preventDefault()
        Promise.resolve(openExternalUrl(url)).catch((error: unknown) => {
          console.error('Could not open external card link', error)
        })
      }
      return (
        <a href={url} onClick={openLink} rel="noopener noreferrer">
          {children}
        </a>
      )
    },
    img({ alt }) {
      return (
        <span className="dara-markdown-inert-image">
          {alt ? `Image: ${alt}` : 'External image unavailable'}
        </span>
      )
    },
    p({ children }) {
      const values = Children.toArray(children)
      const reference =
        values.length === 1 && typeof values[0] === 'string'
          ? parseImageReferenceToken(values[0])
          : null
      if (reference) {
        return (
          <figure
            className="dara-markdown-image"
            style={{ width: `${reference.displayWidthPercent}%` }}
          >
            <img
              alt="Pasted card image"
              src={localMediaUrl(reference.imageId)}
            />
          </figure>
        )
      }
      return <p>{children}</p>
    },
    table({ children }) {
      return (
        <div className="dara-markdown-table-scroll" tabIndex={0}>
          <table>{children}</table>
        </div>
      )
    },
  }
}

interface ErrorBoundaryProps {
  children: ReactNode
  source: string
}

interface ErrorBoundaryState {
  failed: boolean
}

class MarkdownErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { failed: false }

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Card Markdown rendering failed', error, info)
  }

  render() {
    if (this.state.failed) {
      return (
        <div
          aria-label="Card content could not be rendered"
          className="dara-markdown dara-markdown-error"
          role="note"
        >
          <p>Formatting failed. Showing the original source.</p>
          <pre>{this.props.source}</pre>
        </div>
      )
    }
    return this.props.children
  }
}
