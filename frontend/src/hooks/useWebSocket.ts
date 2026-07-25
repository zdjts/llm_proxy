import { useEffect, useRef, useState, useCallback } from 'react';
import type { LiveRequestEvent } from '@/types';

export function useLiveSocket() {
  const [connected, setConnected] = useState(false);
  const [events, setEvents] = useState<LiveRequestEvent[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const connect = useCallback(() => {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/admin/live`);

    ws.onopen = () => {
      setConnected(true);
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current);
    };

    ws.onmessage = (evt) => {
      try {
        const e: LiveRequestEvent = JSON.parse(evt.data);
        if (e.event_type === 'lag') return;
        setEvents(prev => [e, ...prev].slice(0, 200));
      } catch {}
    };

    ws.onclose = () => {
      setConnected(false);
      reconnectTimer.current = setTimeout(connect, 3000);
    };

    ws.onerror = () => ws.close();

    wsRef.current = ws;
  }, []);

  useEffect(() => {
    connect();
    return () => {
      if (wsRef.current) wsRef.current.close();
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current);
    };
  }, [connect]);

  const clear = useCallback(() => setEvents([]), []);

  return { connected, events, clear };
}
