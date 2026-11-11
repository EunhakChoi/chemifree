import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { STOP_DETAIL, stopTitle, type ComputeStop } from '@/lib/compute-stop'

interface ComputeStopNoticeProps {
  stop: ComputeStop
  /** Runs the operation again. */
  onRetry: () => void
  disabled?: boolean
  retryLabel?: string
  testId?: string
}

/**
 * Shows a stopped computation with its title, the statement that no partial result is shown, and
 * a retry. A stop changed nothing, so the notice uses the default alert look, not the destructive one.
 */
export function ComputeStopNotice({ stop, onRetry, disabled, retryLabel = 'Try again', testId = 'compute-stop' }: ComputeStopNoticeProps) {
  return (
    <Alert data-testid={testId}>
      <AlertTitle>{stopTitle(stop)}</AlertTitle>
      <AlertDescription>{STOP_DETAIL}</AlertDescription>
      <AlertAction>
        <Button size="sm" variant="outline" disabled={disabled} onClick={onRetry}>
          {retryLabel}
        </Button>
      </AlertAction>
    </Alert>
  )
}
