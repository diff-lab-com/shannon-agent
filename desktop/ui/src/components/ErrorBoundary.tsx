import { Component, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'

interface Props {
  children: ReactNode
  fallback?: ReactNode
  /**
   * Pane-level mode (R3-V-01): a friendly i18n title + description + a
   * single Retry button, with the raw error message demoted into a collapsed
   * 「技术详情」<details> block. App-level boundaries keep the historical
   * layout (title + raw message + Try Again / Reload) — the raw string is
   * developer-facing and must not be the pane's first sentence.
   */
  pane?: boolean
}

interface State {
  error: Error | null
}

// Translation function type
type TranslationFunc = (id: string) => string

interface ErrorBoundaryInnerProps extends Props {
  t: TranslationFunc
}

class ErrorBoundaryInner extends Component<ErrorBoundaryInnerProps, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  render() {
    const { t } = this.props
    if (this.state.error) {
      if (this.props.fallback) return this.props.fallback
      if (this.props.pane) {
        const message =
          this.state.error.message || this.state.error.toString() || String(this.state.error)
        return (
          <div
            className="flex flex-col items-center justify-center py-xxl text-center"
            role="alert"
            data-testid="pane-error"
          >
            <span className="material-symbols-outlined icon-2xl text-error mb-md">error</span>
            <h3 className="font-headline-md text-on-surface mb-sm">{t('errorBoundary.paneTitle')}</h3>
            <p className="text-body-sm text-on-surface-variant max-w-md mb-lg">
              {t('errorBoundary.paneDescription')}
            </p>
            <Button onClick={() => this.setState({ error: null })} data-testid="pane-error-retry">
              {t('errorBoundary.paneRetry')}
            </Button>
            {/* R3-V-01: the raw exception string is developer-facing — keep it
                in the DOM for support but collapsed by default, never as the
                pane's headline. */}
            <details className="mt-lg max-w-md text-left" data-testid="pane-error-details">
              <summary className="cursor-pointer text-body-sm text-on-surface-variant">
                {t('errorBoundary.technicalDetails')}
              </summary>
              <pre className="mt-xs px-md py-sm rounded-lg bg-surface-container-high text-body-xs text-on-surface-variant whitespace-pre-wrap break-words max-w-md">
                {message}
              </pre>
            </details>
          </div>
        )
      }
      return (
        <div className="flex flex-col items-center justify-center py-xxl text-center" role="alert">
          <span className="material-symbols-outlined icon-2xl text-error mb-md">error</span>
          <h3 className="font-headline-md text-on-surface mb-sm">{t('errorBoundary.title')}</h3>
          <p className="text-body-sm text-on-surface-variant max-w-md mb-lg">{this.state.error.message}</p>
          <Button onClick={() => this.setState({ error: null })}>{t('errorBoundary.tryAgain')}</Button>
          <Button variant="outline" onClick={() => window.location.reload()}>{t('errorBoundary.reloadPage')}</Button>
        </div>
      )
    }
    return this.props.children
  }
}

// Wrapper functional component that uses useIntl hook and passes it down
import { useIntl } from 'react-intl'

export function ErrorBoundary({ children, fallback, pane }: Props) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return <ErrorBoundaryInner t={t} fallback={fallback} pane={pane}>{children}</ErrorBoundaryInner>
}
