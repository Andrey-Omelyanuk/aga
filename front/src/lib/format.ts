export function escapeHtml(input: unknown): string {
  return String(input)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString();
}