import { Component, type ReactNode } from 'react'

interface Props {
  children: ReactNode
  fallback?: ReactNode
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
      return (
        <div className="flex flex-col items-center justify-center py-xxl text-center" role="alert">
          <span className="material-symbols-outlined icon-2xl text-error mb-md">error</span>
          <h3 className="font-headline-md text-on-surface mb-sm">{t('errorBoundary.title')}</h3>
          <p className="text-body-sm text-on-surface-variant max-w-md mb-lg">{this.state.error.message}</p>
          <button className="px-md py-sm bg-primary text-on-primary rounded-xl font-label-md cursor-pointer" onClick={() => this.setState({ error: null })}>{t('errorBoundary.tryAgain')}</button>
          <button className="px-md py-sm border border-outline-variant text-on-surface rounded-xl font-label-md cursor-pointer hover:bg-surface-container transition-colors" onClick={() => window.location.reload()}>{t('errorBoundary.reloadPage')}</button>
        </div>
      )
    }
    return this.props.children
  }
}

// Wrapper functional component that uses useIntl hook and passes it down
import { useIntl } from 'react-intl'

export function ErrorBoundary({ children, fallback }: Props) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return <ErrorBoundaryInner t={t} fallback={fallback}>{children}</ErrorBoundaryInner>
}
