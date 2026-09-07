const WS_BASE = process.env.NEXT_PUBLIC_API_URL?.replace(/^http/, "ws") || "ws://localhost:8080";

export function createBuildLogSocket(
  buildId: string,
  onMessage: (data: string) => void,
  onClose?: () => void
): WebSocket {
  const ws = new WebSocket(`${WS_BASE}/api/v1/ws/builds/${buildId}/logs`);

  ws.onmessage = (event) => {
    onMessage(event.data);
  };

  ws.onclose = () => {
    onClose?.();
  };

  return ws;
}

export function createBuildStatusSocket(
  buildId: string,
  onMessage: (data: string) => void,
  onClose?: () => void
): WebSocket {
  const ws = new WebSocket(`${WS_BASE}/api/v1/ws/builds/${buildId}/status`);

  ws.onmessage = (event) => {
    onMessage(event.data);
  };

  ws.onclose = () => {
    onClose?.();
  };

  return ws;
}
