import { useCallback, useEffect, useState } from 'react'

/** Polls `load` every `ms`, keeping the last good result and the last error. */
export function usePoll<T>(load: () => Promise<T>, ms: number) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const refresh = useCallback(() => {
    load().then((d) => { setData(d); setError(null) }).catch((e) => setError(String(e.message ?? e)))
  }, [load])
  useEffect(() => {
    refresh()
    const t = setInterval(refresh, ms)
    return () => clearInterval(t)
  }, [refresh, ms])
  return { data, error, refresh, setData }
}
