import { useCallback, useEffect, useRef, useState } from "react";

export type Loaded<T> = {
  data: T | null;
  error: unknown;
  loading: boolean;
  reload: () => void;
};

// Fetch on mount and whenever `key` changes; `reload` refetches in place. A
// `pollMs` refetches on that interval without clearing the current data, so a
// phase change from ECS shows up while the page is open.
export function useLoad<T>(fn: () => Promise<T>, key: string, pollMs?: number): Loaded<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let alive = true;
    setLoading(true);
    fnRef
      .current()
      .then((d) => {
        if (!alive) return;
        setData(d);
        setError(null);
      })
      .catch((e) => alive && setError(e))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [key, tick]);

  useEffect(() => {
    if (!pollMs) return;
    const id = setInterval(() => {
      fnRef
        .current()
        .then((d) => {
          setData(d);
          setError(null);
        })
        .catch((e) => setError(e));
    }, pollMs);
    return () => clearInterval(id);
  }, [pollMs, key]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { data, error, loading, reload };
}

// A write in flight: one at a time, with its error kept until the next attempt.
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const run = useCallback(async <T,>(fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(e);
      return undefined;
    } finally {
      setBusy(false);
    }
  }, []);
  return { busy, error, run, clear: () => setError(null) };
}
