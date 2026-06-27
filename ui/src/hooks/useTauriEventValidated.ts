import { useEffect, useRef } from 'react'
import { listen, type UnlistenFn, type EventCallback } from '@tauri-apps/api/event'
import Ajv from 'ajv'
import schema from '../schema/events.schema.json'

const EVENT_TO_PAYLOAD: Record<string, string> = {
  'query:text': 'QueryTextPayload',
  'query:tool-start': 'ToolStartPayload',
  'query:tool-result': 'ToolResultPayload',
  'query:tool-progress': 'ToolProgressPayload',
  'query:thinking': 'ThinkingPayload',
  'query:usage': 'UsagePayload',
  'query:completed': 'QueryCompletedPayload',
  'query:failed': 'QueryFailedPayload',
  'query:cancelled': 'QueryCancelledPayload',
  'permission-request': 'PermissionRequest',
  'session-loaded': 'SessionLoaded',
  'config-updated': 'ConfigUpdatedPayload',
  'background-task-update': 'BackgroundTaskUpdate',
  'update-available': 'UpdateAvailablePayload',
  'update-progress': 'UpdateProgressPayload',
  'task:step': 'TaskStepPayload',
  'task:retry': 'TaskRetryPayload',
}

const ajv = new Ajv({ allErrors: true, strict: false })
const validators: Record<string, ReturnType<Ajv['getSchema']>> = {}

function getValidator(event: string) {
  if (validators[event]) return validators[event]
  const schemaKey = EVENT_TO_PAYLOAD[event]
  if (!schemaKey) return null
  const subSchema = (schema as Record<string, unknown>)[schemaKey]
  if (!subSchema) return null
  const validate = ajv.compile(subSchema as object)
  validators[event] = validate
  return validate
}

function useDevSchemaCheck<T>(event: string, payload: T) {
  if (!import.meta.env.DEV) return
  const validate = getValidator(event)
  if (!validate) return
  if (!validate(payload)) {
    const errors = validate.errors
      ?.map((e) => `${e.instancePath || '<root>'} ${e.message ?? ''}`.trim())
      .join('; ')
    console.warn(
      `[schema] event "${event}" payload failed validation: ${errors ?? 'unknown error'}`,
      payload,
    )
  }
}

export function useTauriEventValidated<T>(
  event: string,
  handler: EventCallback<T>,
) {
  const handlerRef = useRef(handler)
  handlerRef.current = handler

  useEffect(() => {
    let unlisten: UnlistenFn | undefined
    let cancelled = false
    listen<T>(event, (e) => {
      if (cancelled) return
      useDevSchemaCheck(event, e.payload)
      handlerRef.current(e)
    })
      .then((fn) => {
        if (cancelled) {
          fn()
        } else {
          unlisten = fn
        }
      })
      .catch(() => {})
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [event])
}
