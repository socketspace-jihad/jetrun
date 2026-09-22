const WS_BASE = process.env.NEXT_PUBLIC_API_URL?.replace(/^http/, "ws") || "ws://localhost:9005";

export function createStepLogSocket(
  buildId: string,
  stepId: string,
  onMessage: (line: string) => void,
  onClose?: () => void
): WebSocket {
  const ws = new WebSocket(`${WS_BASE}/api/v1/ws/builds/${buildId}/logs/${stepId}`);

  ws.onmessage = (event) => {
    onMessage(event.data);
  };

  ws.onclose = () => {
    onClose?.();
  };

  ws.onerror = () => {
    onClose?.();
  };

  return ws;
}
